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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::builtin::mediaplayer::state::MediaPlayerState;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, RwLock};
    use strom_types::block::PropertyType;
    use strom_types::PropertyValue;
    use uuid::Uuid;

    #[test]
    fn test_normalize_uri_file_scheme() {
        let media_path = std::path::Path::new("/media");
        let uri = "file:///path/to/video.mp4";
        assert_eq!(normalize_uri(uri, media_path), uri);
    }

    #[test]
    fn test_normalize_uri_http_scheme() {
        let media_path = std::path::Path::new("/media");
        let uri = "http://example.com/video.mp4";
        assert_eq!(normalize_uri(uri, media_path), uri);
    }

    #[test]
    fn test_normalize_uri_https_scheme() {
        let media_path = std::path::Path::new("/media");
        let uri = "https://example.com/video.mp4";
        assert_eq!(normalize_uri(uri, media_path), uri);
    }

    #[test]
    fn test_normalize_uri_relative_path() {
        let media_path = std::path::Path::new("/media");
        let path = "video.mp4";
        let result = normalize_uri(path, media_path);
        assert!(result.starts_with("file://"), "Should have file:// prefix");
        assert!(result.ends_with("video.mp4"), "Should end with filename");
    }

    #[test]
    fn test_normalize_uri_absolute_path() {
        let media_path = std::path::Path::new("/media");
        #[cfg(not(target_os = "windows"))]
        {
            let path = "/tmp/video.mp4";
            let result = normalize_uri(path, media_path);
            assert_eq!(result, "file:///tmp/video.mp4");
        }
        #[cfg(target_os = "windows")]
        {
            let path = "C:\\temp\\video.mp4";
            let result = normalize_uri(path, media_path);
            assert!(
                result.starts_with("file:///"),
                "Windows path should get file:/// prefix"
            );
            assert!(result.contains("video.mp4"), "Should contain filename");
        }
    }

    #[test]
    fn test_media_player_registry_basic() {
        use gstreamer as gst;

        let registry = state::MediaPlayerRegistry::new();

        let key = MediaPlayerKey {
            flow_id: Uuid::new_v4(),
            block_id: "test_block".to_string(),
        };

        assert!(!registry.contains(&key));
        assert!(registry.get(&key).is_none());

        let weak_ref = gst::glib::WeakRef::new();
        let state = Arc::new(MediaPlayerState {
            instance_id: Uuid::new_v4(),
            source_element: weak_ref,
            internal_pipeline: RwLock::new(None),
            video_appsrc: None,
            audio_appsrc: None,
            playlist: RwLock::new(vec!["file1.mp4".to_string(), "file2.mp4".to_string()]),
            current_index: AtomicUsize::new(0),
            is_paused: AtomicBool::new(false),
            loop_playlist: AtomicBool::new(true),
            block_id: "test_block".to_string(),
            flow_id: key.flow_id,
            video_linked: AtomicBool::new(false),
            audio_linked: AtomicBool::new(false),
            decode: false,
            sync: true,
            media_path: std::path::PathBuf::from("/media"),
            ts_offset: Arc::new(std::sync::atomic::AtomicI64::new(i64::MIN)),
            main_pipeline: gst::glib::WeakRef::new(),
        });

        registry.register(key.clone(), Arc::clone(&state));
        assert!(registry.contains(&key));
        assert!(registry.get(&key).is_some());

        registry.unregister(&key);
        assert!(!registry.contains(&key));
        assert!(registry.get(&key).is_none());
    }

    #[test]
    fn test_media_player_state_playlist() {
        use gstreamer as gst;

        let weak_ref = gst::glib::WeakRef::new();
        let state = MediaPlayerState {
            instance_id: Uuid::new_v4(),
            source_element: weak_ref,
            internal_pipeline: RwLock::new(None),
            video_appsrc: None,
            audio_appsrc: None,
            playlist: RwLock::new(vec![]),
            current_index: AtomicUsize::new(0),
            is_paused: AtomicBool::new(false),
            loop_playlist: AtomicBool::new(true),
            block_id: "test".to_string(),
            flow_id: Uuid::new_v4(),
            video_linked: AtomicBool::new(false),
            audio_linked: AtomicBool::new(false),
            decode: false,
            sync: true,
            media_path: std::path::PathBuf::from("/media"),
            ts_offset: Arc::new(std::sync::atomic::AtomicI64::new(i64::MIN)),
            main_pipeline: gst::glib::WeakRef::new(),
        };

        assert_eq!(state.playlist_len(), 0);
        assert!(state.current_file().is_none());

        state.set_playlist(vec![
            "file1.mp4".to_string(),
            "file2.mp4".to_string(),
            "file3.mp4".to_string(),
        ]);
        assert_eq!(state.playlist_len(), 3);
        assert_eq!(state.current_file(), Some("file1.mp4".to_string()));
    }

    #[test]
    fn test_media_player_state_string() {
        use gstreamer as gst;

        let weak_ref = gst::glib::WeakRef::new();
        let state = MediaPlayerState {
            instance_id: Uuid::new_v4(),
            source_element: weak_ref,
            internal_pipeline: RwLock::new(None),
            video_appsrc: None,
            audio_appsrc: None,
            playlist: RwLock::new(vec![]),
            current_index: AtomicUsize::new(0),
            is_paused: AtomicBool::new(false),
            loop_playlist: AtomicBool::new(true),
            block_id: "test".to_string(),
            flow_id: Uuid::new_v4(),
            video_linked: AtomicBool::new(false),
            audio_linked: AtomicBool::new(false),
            decode: false,
            sync: true,
            media_path: std::path::PathBuf::from("/media"),
            ts_offset: Arc::new(std::sync::atomic::AtomicI64::new(i64::MIN)),
            main_pipeline: gst::glib::WeakRef::new(),
        };

        assert_eq!(state.state_string(), "stopped");

        state.set_playlist(vec!["file.mp4".to_string()]);
        assert_eq!(state.state_string(), "playing");

        state.is_paused.store(true, Ordering::SeqCst);
        assert_eq!(state.state_string(), "paused");
    }

    #[test]
    fn test_media_player_definition() {
        let def = definition::media_player_definition();

        assert_eq!(def.id, "builtin.media_player");
        assert_eq!(def.name, "Media Player");
        assert_eq!(def.category, "Inputs");
        assert!(def.built_in);

        assert_eq!(def.exposed_properties.len(), 4);

        let decode_prop = def.exposed_properties.iter().find(|p| p.name == "decode");
        assert!(decode_prop.is_some());
        let decode_prop = decode_prop.unwrap();
        assert!(matches!(decode_prop.property_type, PropertyType::Bool));
        assert!(matches!(
            decode_prop.default_value,
            Some(PropertyValue::Bool(false))
        ));

        let loop_prop = def
            .exposed_properties
            .iter()
            .find(|p| p.name == "loop_playlist");
        assert!(loop_prop.is_some());

        let sync_prop = def.exposed_properties.iter().find(|p| p.name == "sync");
        assert!(sync_prop.is_some());
        assert!(matches!(
            sync_prop.unwrap().default_value,
            Some(PropertyValue::Bool(true))
        ));

        assert_eq!(def.external_pads.inputs.len(), 0);
        assert_eq!(def.external_pads.outputs.len(), 2);

        let video_pad = def
            .external_pads
            .outputs
            .iter()
            .find(|p| p.name == "video_out");
        assert!(video_pad.is_some());

        let audio_pad = def
            .external_pads
            .outputs
            .iter()
            .find(|p| p.name == "audio_out");
        assert!(audio_pad.is_some());
    }
}
