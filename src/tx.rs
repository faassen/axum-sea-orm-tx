use std::ops::Deref;
use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use sea_orm::DatabaseTransaction;

use crate::error::Error;

/// The transaction of the request, as an extractor.
///
/// You use this extractor in your Axum handlers so you can access the database.
///
/// It derefs to [`DatabaseTransaction`], so `&*tx` goes where a
/// [`ConnectionTrait`](sea_orm::ConnectionTrait) is expected.
#[derive(Clone)]
pub struct Tx(pub(crate) Arc<DatabaseTransaction>);

impl Deref for Tx {
    type Target = DatabaseTransaction;

    fn deref(&self) -> &DatabaseTransaction {
        &self.0
    }
}

/// We can access the `Arc` of the transaction directly, so that we can pass it
/// to a per-request store that needs a reference to it.
///
/// The middleware can only commit or roll back if nothing else uses it anymore,
/// but if stores are constructed per-request this store is dropped in the end.
impl From<Tx> for Arc<DatabaseTransaction> {
    fn from(tx: Tx) -> Arc<DatabaseTransaction> {
        tx.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Tx {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Error> {
        parts
            .extensions
            .get::<Tx>()
            .cloned()
            .ok_or(Error::MissingMiddleware)
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{Database, TransactionTrait};

    use super::*;

    #[tokio::test]
    async fn gives_its_transaction() {
        let database_connection = Database::connect("sqlite::memory:").await.unwrap();
        let database_transaction = Arc::new(database_connection.begin().await.unwrap());
        let tx = Tx(database_transaction.clone());

        let given_transaction = Arc::<DatabaseTransaction>::from(tx);

        assert!(Arc::ptr_eq(&given_transaction, &database_transaction));
    }
}
