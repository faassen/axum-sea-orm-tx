use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use sea_orm::DbErr;

/// What can go wrong with the transaction of a request.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The [`Tx`](crate::Tx) extractor was used on a route without the
    /// middleware.
    #[error("no transaction for the request; is the middleware added to the route?")]
    MissingMiddleware,
    /// Something still held the [`Tx`](crate::Tx) after the handler
    /// responded, so the transaction couldn't be committed.
    #[error("the transaction was still held after the handler responded")]
    TransactionHeld,
    /// Beginning, committing or rolling back the transaction failed.
    #[error(transparent)]
    Database(#[from] DbErr),
}

impl IntoResponse for Error {
    /// A server error, which doesn't tell the client what went wrong; the
    /// log does.
    fn into_response(self) -> Response {
        tracing::error!(error = %self, "request transaction failed");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}
