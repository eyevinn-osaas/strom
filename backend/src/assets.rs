//! Embedded static assets for the frontend.

use axum::{
    body::Body,
    http::{header, Response, StatusCode, Uri},
    response::IntoResponse,
};
use std::sync::OnceLock;

// Include the generated assets code
include!(concat!(env!("OUT_DIR"), "/assets.rs"));

// Lazy-initialized asset map
static ASSETS: OnceLock<std::collections::HashMap<&'static str, EmbeddedAsset>> = OnceLock::new();

fn assets() -> &'static std::collections::HashMap<&'static str, EmbeddedAsset> {
    ASSETS.get_or_init(get_assets)
}

/// Serve embedded static files
pub async fn serve_static(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    // If path is empty, serve index.html
    let path = if path.is_empty() || path == "index.html" {
        "index.html"
    } else {
        path
    };

    match assets().get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content.data))
                .unwrap()
        }
        None => {
            // If file not found, try to serve index.html for SPA routing
            if let Some(index) = assets().get("index.html") {
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/html")
                    .body(Body::from(index.data))
                    .unwrap()
            } else {
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("404 Not Found"))
                    .unwrap()
            }
        }
    }
}
