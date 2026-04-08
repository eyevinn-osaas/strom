//! Media player block for file playback with playlist support.
//!
//! Architecture: an **internal pipeline** (owned by the block) handles decoding/parsing
//! and paces buffers through `clocksync`. An appsink→appsrc bridge feeds the main
//! pipeline, isolating downstream from seeks, file switches, and EOS.
//!
//! Supports two modes:
//! - **Decode** (`decode=true`): `uridecodebin` → raw video/audio
//! - **Passthrough** (`decode=false`): `urisourcebin` → encoded elementary streams

mod bridge;
mod builder;
mod definition;
mod state;

pub use builder::MediaPlayerBuilder;
pub use definition::get_blocks;
pub use state::{MediaPlayerKey, MediaPlayerState, MEDIA_PLAYER_REGISTRY};

use std::path::Path;
use tracing::debug;

/// Normalize a file path to a proper URI.
///
/// Converts relative paths to absolute file:// URIs resolved against `media_path`.
/// Passes through URIs that already have a scheme (file://, http://, https://).
///
/// Relative paths are resolved relative to `media_path` (the configured media directory).
/// Legacy paths starting with `./media/` have that prefix stripped before resolution.
pub fn normalize_uri(path: &str, media_path: &Path) -> String {
    // If it already has a scheme, pass through
    if path.starts_with("file://") || path.starts_with("http://") || path.starts_with("https://") {
        return path.to_string();
    }

    // Strip legacy "./media/" prefix
    let clean_path = path
        .strip_prefix("./media/")
        .or_else(|| path.strip_prefix("media/"))
        .unwrap_or(path);

    // Check if it's an absolute path
    let file_path = if Path::new(clean_path).is_absolute() {
        std::path::PathBuf::from(clean_path)
    } else {
        // Resolve against media_path
        media_path.join(clean_path)
    };

    // Try to canonicalize the path (resolves symlinks, normalizes ..)
    let resolved = if file_path.exists() {
        file_path
            .canonicalize()
            .unwrap_or_else(|_| file_path.clone())
    } else {
        file_path
    };

    let uri = format!("file://{}", resolved.display());
    debug!("Normalized '{}' → '{}'", path, uri);
    uri
}
