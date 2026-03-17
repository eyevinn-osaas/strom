//! WHIP API types shared between backend and frontend.

use serde::Deserialize;

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// A client-side log entry sent from the WHIP ingest page.
#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ClientLogEntry {
    pub msg: String,
    pub level: Option<String>,
}
