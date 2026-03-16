//! Custom JSON extractor with structured error responses.
//!
//! Wraps Axum's [`Json`] extractor so that deserialization failures are returned
//! as JSON matching [`ErrorResponse`] instead of Axum's default plaintext.
//!
//! # When to use
//!
//! Use [`JsonBody<T>`] instead of [`axum::Json<T>`] in handlers that accept
//! JSON request bodies from external clients. This ensures callers always
//! receive machine-readable error responses when they send malformed JSON.
//!
//! Existing handlers that already use `axum::Json<T>` continue to work
//! unchanged; they can be migrated to `JsonBody<T>` incrementally.

use axum::{
    extract::{rejection::JsonRejection, FromRequest, Request},
    response::{IntoResponse, Response},
    Json,
};
use serde::de::DeserializeOwned;
use strom_types::api::ErrorResponse;

/// A JSON extractor that returns structured [`ErrorResponse`] on failure.
pub struct JsonBody<T>(pub T);

impl<S, T> FromRequest<S> for JsonBody<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(JsonBody(value)),
            Err(rejection) => {
                let status = rejection.status();
                let error_response =
                    ErrorResponse::with_details("Invalid request body", rejection.body_text());

                Err((status, Json(error_response)).into_response())
            }
        }
    }
}
