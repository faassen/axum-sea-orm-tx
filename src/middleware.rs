use std::sync::Arc;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use sea_orm::{DatabaseConnection, DatabaseTransaction, TransactionTrait};

use crate::error::Error;
use crate::tx::Tx;

/// Runs the request in a new transaction on the database connection. For
/// [`axum::middleware::from_fn_with_state`], with the connection as state.
pub async fn transaction(
    State(database_connection): State<DatabaseConnection>,
    request: Request,
    next: Next,
) -> Result<Response, Error> {
    let database_transaction = database_connection.begin().await?;
    run(database_transaction, request, next).await
}

/// Runs the request in a savepoint of the test's transaction. Multiple requests
/// can share a single transaction so that new requests can see the effects of
/// previousy ones. All changes are rolled back in the end, so that the test
/// database is in a consistent state in the end. For
/// [`axum::middleware::from_fn_with_state`], with the test's transaction as
/// state.
pub async fn test_transaction(
    State(test_transaction): State<Arc<DatabaseTransaction>>,
    request: Request,
    next: Next,
) -> Result<Response, Error> {
    let database_transaction = test_transaction.begin().await?;
    run(database_transaction, request, next).await
}

/// Runs the request with the transaction for its [`Tx`], and commits the
/// transaction if the response is a success or a redirect, or rolls it back
/// otherwise.
async fn run(
    database_transaction: DatabaseTransaction,
    mut request: Request,
    next: Next,
) -> Result<Response, Error> {
    let database_transaction = Arc::new(database_transaction);
    request
        .extensions_mut()
        .insert(Tx(database_transaction.clone()));
    let response = next.run(request).await;
    let status = response.status();
    let succeeded = status.is_success() || status.is_redirection();
    if let Ok(database_transaction) = Arc::try_unwrap(database_transaction) {
        if succeeded {
            database_transaction.commit().await?;
        } else {
            database_transaction.rollback().await?;
        }
    } else if succeeded {
        return Err(Error::TransactionHeld);
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::http::StatusCode;
    use axum::middleware::from_fn_with_state;
    use axum::response::Redirect;
    use axum::routing::{get, post};
    use sea_orm::{
        ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbErr, Statement,
    };
    use tower::ServiceExt;

    use super::*;

    async fn database_connection() -> DatabaseConnection {
        // A single connection, as each connection to an in-memory database
        // has a database of its own.
        let mut connect_options = ConnectOptions::new("sqlite::memory:");
        connect_options.max_connections(1).min_connections(1);
        let database_connection = Database::connect(connect_options).await.unwrap();
        database_connection
            .execute_unprepared("CREATE TABLE item (id INTEGER PRIMARY KEY)")
            .await
            .unwrap();
        database_connection
    }

    async fn insert_item(connection: &impl ConnectionTrait) -> Result<(), DbErr> {
        connection
            .execute_unprepared("INSERT INTO item DEFAULT VALUES")
            .await?;
        Ok(())
    }

    async fn item_count(connection: &impl ConnectionTrait) -> i64 {
        let statement = Statement::from_string(
            connection.get_database_backend(),
            "SELECT COUNT(*) AS count FROM item",
        );
        let query_result = connection.query_one_raw(statement).await.unwrap().unwrap();
        query_result.try_get("", "count").unwrap()
    }

    /// Routes that insert an item and respond with the status, and one that
    /// responds with how many items there are.
    fn item_router() -> Router {
        Router::new()
            .route(
                "/ok",
                post(|tx: Tx| async move {
                    insert_item(&*tx).await.unwrap();
                    StatusCode::OK
                }),
            )
            .route(
                "/redirect",
                post(|tx: Tx| async move {
                    insert_item(&*tx).await.unwrap();
                    Redirect::to("/count")
                }),
            )
            .route(
                "/not-found",
                post(|tx: Tx| async move {
                    insert_item(&*tx).await.unwrap();
                    StatusCode::NOT_FOUND
                }),
            )
            .route(
                "/server-error",
                post(|tx: Tx| async move {
                    insert_item(&*tx).await.unwrap();
                    StatusCode::INTERNAL_SERVER_ERROR
                }),
            )
            .route(
                "/count",
                get(|tx: Tx| async move { item_count(&*tx).await.to_string() }),
            )
    }

    async fn send(router: &Router, method: &str, uri: &str) -> Response {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        router.clone().oneshot(request).await.unwrap()
    }

    async fn body_text(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn commits_on_success() {
        let database_connection = database_connection().await;
        let router =
            item_router().layer(from_fn_with_state(database_connection.clone(), transaction));

        let response = send(&router, "POST", "/ok").await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(item_count(&database_connection).await, 1);
    }

    #[tokio::test]
    async fn commits_on_redirect() {
        let database_connection = database_connection().await;
        let router =
            item_router().layer(from_fn_with_state(database_connection.clone(), transaction));

        let response = send(&router, "POST", "/redirect").await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(item_count(&database_connection).await, 1);
    }

    #[tokio::test]
    async fn rolls_back_on_client_error() {
        let database_connection = database_connection().await;
        let router =
            item_router().layer(from_fn_with_state(database_connection.clone(), transaction));

        let response = send(&router, "POST", "/not-found").await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(item_count(&database_connection).await, 0);
    }

    #[tokio::test]
    async fn rolls_back_on_server_error() {
        let database_connection = database_connection().await;
        let router =
            item_router().layer(from_fn_with_state(database_connection.clone(), transaction));

        let response = send(&router, "POST", "/server-error").await;

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(item_count(&database_connection).await, 0);
    }

    #[tokio::test]
    async fn test_transaction_shares_writes_until_dropped() {
        let database_connection = database_connection().await;
        let test_transaction_state = Arc::new(database_connection.begin().await.unwrap());
        let router = item_router().layer(from_fn_with_state(
            test_transaction_state.clone(),
            test_transaction,
        ));

        send(&router, "POST", "/ok").await;
        send(&router, "POST", "/server-error").await;
        let response = send(&router, "GET", "/count").await;

        assert_eq!(body_text(response).await, "1");
        drop(router);
        drop(test_transaction_state);
        assert_eq!(item_count(&database_connection).await, 0);
    }

    #[tokio::test]
    async fn tx_without_middleware_is_server_error() {
        let router = item_router();

        let response = send(&router, "GET", "/count").await;

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
