//! Media player runtime state, global registry, and lifecycle methods.

use super::normalize_uri;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, RwLock};
use strom_types::FlowId;
use tracing::{debug, error, info};
use uuid::Uuid;

/// Global registry of media player instances for API access.
pub static MEDIA_PLAYER_REGISTRY: LazyLock<MediaPlayerRegistry> =
    LazyLock::new(MediaPlayerRegistry::new);

/// Registry key for looking up media player instances.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct MediaPlayerKey {
    pub flow_id: FlowId,
    pub block_id: String,
}

/// Runtime state for a media player instance.
pub struct MediaPlayerState {
    /// Unique instance ID (to detect stale timers after restart)
    pub instance_id: Uuid,
    /// Weak reference to the source element (uridecodebin or urisourcebin) in the internal pipeline
    pub source_element: gst::glib::WeakRef<gst::Element>,
    /// The isolated internal pipeline (owned by this block)
    pub internal_pipeline: RwLock<Option<gst::Pipeline>>,
    /// Video appsrc in the main pipeline (bridge target)
    pub video_appsrc: Option<gst_app::AppSrc>,
    /// Audio appsrc in the main pipeline (bridge target)
    pub audio_appsrc: Option<gst_app::AppSrc>,
    /// Current playlist of file URIs
    pub playlist: RwLock<Vec<String>>,
    /// Current file index
    pub current_index: AtomicUsize,
    /// Whether playback is paused
    pub is_paused: AtomicBool,
    /// Whether to loop the playlist
    pub loop_playlist: AtomicBool,
    /// Block ID for event broadcasting
    pub block_id: String,
    /// Flow ID for event broadcasting
    pub flow_id: FlowId,
    /// Whether video pad has been linked (reset on file switch)
    pub video_linked: AtomicBool,
    /// Whether audio pad has been linked (reset on file switch)
    pub audio_linked: AtomicBool,
    /// Whether to decode streams (true) or pass through encoded (false)
    pub decode: bool,
    /// Whether clocksync pacing is enabled
    pub sync: bool,
    /// Configured media files directory (for resolving relative playlist paths)
    pub media_path: std::path::PathBuf,
    /// Shared timestamp offset (ns) for the appsink→appsrc bridge.
    /// Computed once from the first buffer: `main_running_time - buffer_pts`.
    /// Set to `i64::MIN` to signal "needs recomputation" (on startup, file switch, resume).
    pub ts_offset: Arc<AtomicI64>,
    /// Weak reference to the main pipeline (for computing running time in the bridge).
    pub main_pipeline: gst::glib::WeakRef<gst::Pipeline>,
}

impl MediaPlayerState {
    /// Get the current file URI, if any.
    pub fn current_file(&self) -> Option<String> {
        let playlist = self.playlist.read().ok()?;
        let index = self.current_index.load(Ordering::SeqCst);
        playlist.get(index).cloned()
    }

    /// Get the playlist length.
    pub fn playlist_len(&self) -> usize {
        self.playlist.read().map(|p| p.len()).unwrap_or(0)
    }

    /// Set the playlist.
    pub fn set_playlist(&self, files: Vec<String>) {
        if let Ok(mut playlist) = self.playlist.write() {
            *playlist = files;
        }
    }

    /// Go to a specific file index.
    pub fn goto(&self, index: usize) -> Result<(), String> {
        let playlist_len = self.playlist_len();
        if index >= playlist_len {
            return Err(format!(
                "Index {} out of range (playlist has {} files)",
                index, playlist_len
            ));
        }
        self.current_index.store(index, Ordering::SeqCst);
        self.load_current_file()
    }

    /// Advance to the next file.
    pub fn next(&self) -> Result<(), String> {
        let playlist_len = self.playlist_len();
        if playlist_len == 0 {
            return Err("Playlist is empty".to_string());
        }

        let current = self.current_index.load(Ordering::SeqCst);
        let next = current + 1;
        if next >= playlist_len {
            if self.loop_playlist.load(Ordering::SeqCst) {
                self.current_index.store(0, Ordering::SeqCst);
            } else {
                return Err("End of playlist".to_string());
            }
        } else {
            self.current_index.store(next, Ordering::SeqCst);
        }
        self.load_current_file()
    }

    /// Go to the previous file.
    pub fn previous(&self) -> Result<(), String> {
        let playlist_len = self.playlist_len();
        if playlist_len == 0 {
            return Err("Playlist is empty".to_string());
        }

        let current = self.current_index.load(Ordering::SeqCst);
        if current == 0 {
            if self.loop_playlist.load(Ordering::SeqCst) {
                self.current_index.store(playlist_len - 1, Ordering::SeqCst);
            } else {
                return Err("Already at start of playlist".to_string());
            }
        } else {
            self.current_index.store(current - 1, Ordering::SeqCst);
        }
        self.load_current_file()
    }

    /// Load the current file into the internal pipeline's source element.
    ///
    /// Removes dynamically-created elements (clocksync, appsink) from the previous
    /// file, then sets the new URI and restarts. The pad-added callback will recreate
    /// the clocksync→appsink chain for the new file's pads.
    fn load_current_file(&self) -> Result<(), String> {
        let file_path = self.current_file().ok_or("No file to load")?;
        let source_element = self
            .source_element
            .upgrade()
            .ok_or("Source element no longer exists")?;

        let uri = normalize_uri(&file_path, &self.media_path);
        info!("Loading file: {}", uri);

        let pipeline_guard = self
            .internal_pipeline
            .read()
            .map_err(|e| format!("Failed to lock internal pipeline: {}", e))?;
        let pipeline = pipeline_guard
            .as_ref()
            .ok_or("Internal pipeline not created")?;

        // Set internal pipeline to READY to flush the old stream
        pipeline
            .set_state(gst::State::Ready)
            .map_err(|e| format!("Failed to set state to Ready: {:?}", e))?;

        // Remove dynamically-created clocksync and appsink elements from previous file.
        // The source element (uridecodebin/urisourcebin) stays — only the bridge chain
        // is recreated by pad-added when the new file starts.
        let dynamic_elements: Vec<gst::Element> = pipeline
            .iterate_elements()
            .into_iter()
            .flatten()
            .filter(|e| e.name() != source_element.name())
            .collect();
        for elem in &dynamic_elements {
            let _ = elem.set_state(gst::State::Null);
        }
        for elem in &dynamic_elements {
            let _ = pipeline.remove(elem);
        }

        // Reset linked flags and timestamp offset so new pads get linked
        // and the bridge recomputes the offset from the first buffer
        self.video_linked.store(false, Ordering::SeqCst);
        self.audio_linked.store(false, Ordering::SeqCst);
        self.ts_offset.store(i64::MIN, Ordering::SeqCst);

        // Set the new URI on source element
        source_element.set_property("uri", &uri);

        // Start playing again
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| format!("Failed to set state to Playing: {:?}", e))?;

        self.is_paused.store(false, Ordering::SeqCst);

        Ok(())
    }

    /// Play the media.
    pub fn play(&self) -> Result<(), String> {
        let pipeline_guard = self
            .internal_pipeline
            .read()
            .map_err(|e| format!("Lock error: {}", e))?;
        let pipeline = pipeline_guard
            .as_ref()
            .ok_or("Internal pipeline not created")?;
        // Reset timestamp offset so the bridge recomputes from the first buffer
        // after resume — prevents accumulated drift from pause duration.
        self.ts_offset.store(i64::MIN, Ordering::SeqCst);
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| format!("Failed to set state to Playing: {:?}", e))?;
        self.is_paused.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Pause the media.
    pub fn pause(&self) -> Result<(), String> {
        let pipeline_guard = self
            .internal_pipeline
            .read()
            .map_err(|e| format!("Lock error: {}", e))?;
        let pipeline = pipeline_guard
            .as_ref()
            .ok_or("Internal pipeline not created")?;
        pipeline
            .set_state(gst::State::Paused)
            .map_err(|e| format!("Failed to set state to Paused: {:?}", e))?;
        self.is_paused.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Seek to a position in nanoseconds.
    ///
    /// Seeks on the internal pipeline's source element. The timestamp offset is reset
    /// so the bridge recomputes it from the first buffer at the new position.
    pub fn seek(&self, position_ns: u64) -> Result<(), String> {
        let secs = position_ns / 1_000_000_000;
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        let secs_rem = secs % 60;
        info!(
            "Seeking to {} ns ({:02}:{:02}:{:02})",
            position_ns, hours, mins, secs_rem
        );

        let source = self
            .source_element
            .upgrade()
            .ok_or("Source element no longer exists")?;

        // Reset timestamp offset so the bridge recomputes from the first buffer
        // after the seek — the file PTS jumps but main pipeline running time doesn't.
        self.ts_offset.store(i64::MIN, Ordering::SeqCst);

        let seek_result = source.seek_simple(
            gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
            gst::ClockTime::from_nseconds(position_ns),
        );

        match seek_result {
            Ok(_) => {
                info!("Seek completed to {} ns", position_ns);
                Ok(())
            }
            Err(e) => {
                error!("Seek failed: {:?}", e);
                Err(format!("Seek failed: {:?}", e))
            }
        }
    }

    /// Get current position in nanoseconds.
    pub fn position(&self) -> Option<u64> {
        if let Some(source) = self.source_element.upgrade() {
            if let Some(position) = source.query_position::<gst::ClockTime>() {
                return Some(position.nseconds());
            }
        }

        // Fallback: query internal pipeline
        if let Ok(guard) = self.internal_pipeline.read() {
            if let Some(ref pipeline) = *guard {
                return pipeline
                    .query_position::<gst::ClockTime>()
                    .map(|t| t.nseconds());
            }
        }
        None
    }

    /// Get duration in nanoseconds.
    pub fn duration(&self) -> Option<u64> {
        // Try internal pipeline first
        if let Ok(guard) = self.internal_pipeline.read() {
            if let Some(ref pipeline) = *guard {
                if let Some(duration) = pipeline.query_duration::<gst::ClockTime>() {
                    return Some(duration.nseconds());
                }
            }
        }

        // Fallback: try querying the source element directly
        if let Some(source) = self.source_element.upgrade() {
            if let Some(duration) = source.query_duration::<gst::ClockTime>() {
                return Some(duration.nseconds());
            }
        }

        None
    }

    /// Get the current playback state as a string.
    pub fn state_string(&self) -> String {
        if self.is_paused.load(Ordering::SeqCst) {
            "paused".to_string()
        } else if self.playlist_len() == 0 {
            "stopped".to_string()
        } else {
            "playing".to_string()
        }
    }
}

impl Drop for MediaPlayerState {
    fn drop(&mut self) {
        if let Ok(guard) = self.internal_pipeline.read() {
            if let Some(ref pipeline) = *guard {
                debug!(
                    "Media Player {}: Stopping internal pipeline on drop",
                    self.block_id
                );
                let _ = pipeline.set_state(gst::State::Null);
            }
        }
    }
}

/// Global registry for media player instances.
pub struct MediaPlayerRegistry {
    players: RwLock<HashMap<MediaPlayerKey, Arc<MediaPlayerState>>>,
}

impl MediaPlayerRegistry {
    pub fn new() -> Self {
        Self {
            players: RwLock::new(HashMap::new()),
        }
    }

    pub fn register(&self, key: MediaPlayerKey, state: Arc<MediaPlayerState>) {
        if let Ok(mut players) = self.players.write() {
            players.insert(key, state);
        }
    }

    pub fn unregister(&self, key: &MediaPlayerKey) {
        if let Ok(mut players) = self.players.write() {
            players.remove(key);
        }
    }

    pub fn get(&self, key: &MediaPlayerKey) -> Option<Arc<MediaPlayerState>> {
        self.players.read().ok()?.get(key).cloned()
    }

    pub fn contains(&self, key: &MediaPlayerKey) -> bool {
        self.players
            .read()
            .ok()
            .map(|p| p.contains_key(key))
            .unwrap_or(false)
    }
}

impl Default for MediaPlayerRegistry {
    fn default() -> Self {
        Self::new()
    }
}
