//! Version information API endpoints.

use axum::Json;

use crate::version::SystemInfo;

/// Get version and build information
///
/// Returns comprehensive version information including:
/// - Package version from Cargo.toml
/// - Git commit hash
/// - Git tag (if on a tagged release)
/// - Git branch name
/// - Working directory status (dirty/clean)
/// - Build timestamp
#[utoipa::path(
    get,
    path = "/api/version",
    tag = "System",
    responses(
        (status = 200, description = "Version information", body = SystemInfo)
    )
)]
pub async fn get_version() -> Json<SystemInfo> {
    Json(crate::version::get())
}
