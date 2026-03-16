//! Custom JSON extractors with structured error responses.
//!
//! Provides two extractors that return JSON [`ErrorResponse`] instead of Axum's
//! default plaintext on failure:
//!
//! - [`JsonBody<T>`] -- handles JSON deserialization errors only.
//! - [`ValidatedJson<T>`] -- handles deserialization errors **and** runs
//!   [`garde::Validate`] on the deserialized value, returning 422 on validation
//!   failure.
//!
//! # When to use
//!
//! | Extractor | Use when |
//! |-----------|----------|
//! | `JsonBody<T>` | The request type does **not** derive `garde::Validate`. |
//! | `ValidatedJson<T>` | The request type derives `garde::Validate`. |
//!
//! Both ensure callers always receive machine-readable JSON error responses.

use axum::{
    extract::{rejection::JsonRejection, FromRequest, Request},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::de::DeserializeOwned;
use strom_types::api::ErrorResponse;

// ---------------------------------------------------------------------------
// JsonBody<T> -- deserialization only
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// ValidatedJson<T> -- deserialization + garde validation
// ---------------------------------------------------------------------------

/// A JSON extractor that returns structured [`ErrorResponse`] on both
/// deserialization failure and [`garde::Validate`] failure.
///
/// On validation failure the response status is `422 Unprocessable Entity`.
pub struct ValidatedJson<T>(pub T);

impl<S, T> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned + garde::Validate,
    <T as garde::Validate>::Context: Default,
    S: Send + Sync,
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let value = match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => value,
            Err(rejection) => {
                let status = rejection.status();
                let error_response =
                    ErrorResponse::with_details("Invalid request body", rejection.body_text());

                return Err((status, Json(error_response)).into_response());
            }
        };

        if let Err(report) = value.validate() {
            let error_response =
                ErrorResponse::with_details("Validation failed", report.to_string());

            return Err((StatusCode::UNPROCESSABLE_ENTITY, Json(error_response)).into_response());
        }

        Ok(ValidatedJson(value))
    }
}
