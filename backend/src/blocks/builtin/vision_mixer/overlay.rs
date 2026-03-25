//! Cairo overlay drawing and shared state for the vision mixer multiview.

use super::layout::OverlayLayout;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime};

/// Global registry of vision mixer overlay states, keyed by block instance ID.
/// Used by the API layer to access overlay state for preview/PGM updates.
fn overlay_states() -> &'static Mutex<HashMap<String, Arc<VisionMixerOverlayState>>> {
    static INSTANCE: OnceLock<Mutex<HashMap<String, Arc<VisionMixerOverlayState>>>> =
        OnceLock::new();
    INSTANCE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register an overlay state for a block instance.
pub fn register_overlay_state(block_id: &str, state: Arc<VisionMixerOverlayState>) {
    if let Ok(mut map) = overlay_states().lock() {
        map.insert(block_id.to_string(), state);
    }
}

/// Get the overlay state for a block instance (if registered).
pub fn get_overlay_state(block_id: &str) -> Option<Arc<VisionMixerOverlayState>> {
    overlay_states().lock().ok()?.get(block_id).cloned()
}

/// Unregister the overlay state for a block instance (call on flow stop).
pub fn unregister_overlay_state(block_id: &str) {
    if let Ok(mut map) = overlay_states().lock() {
        map.remove(block_id);
    }
}

/// Shared state read by the cairooverlay draw callback.
///
/// Updated atomically from the API thread; read lock-free from the streaming thread.
pub struct VisionMixerOverlayState {
    /// Index of the current PGM input.
    pub pgm_input: AtomicUsize,
    /// Index of the current PVW input.
    pub pvw_input: AtomicUsize,
    /// Number of inputs.
    pub num_inputs: usize,
    /// Whether Fade to Black is active.
    pub ftb_active: AtomicBool,
    /// DSK enabled states (one per DSK input, max 4).
    pub dsk_enabled: Vec<AtomicBool>,
    /// Number of DSK inputs.
    pub num_dsk_inputs: usize,
    /// Pre-computed layout (immutable after construction).
    pub layout: OverlayLayout,
    /// Input labels (set at build time, read-only after).
    pub labels: Vec<String>,
    /// Monotonic instant captured at construction for wall-clock derivation.
    instant_base: Instant,
    /// UTC seconds at `instant_base` (no timezone offset applied).
    base_utc_secs: u64,
    /// Local timezone offset in seconds east of UTC. Refreshed periodically for DST changes.
    tz_offset_secs: AtomicI64,
    /// Timezone abbreviation packed as bytes (up to 7 ASCII chars + 1 length byte in MSB).
    tz_abbr_packed: AtomicU64,
    /// Elapsed seconds (from instant_base) when we next refresh timezone info.
    tz_next_refresh: AtomicU64,
}

impl VisionMixerOverlayState {
    pub fn new(
        num_inputs: usize,
        num_dsk_inputs: usize,
        pgm_input: usize,
        pvw_input: usize,
        labels: Vec<String>,
        layout: OverlayLayout,
    ) -> Self {
        let now_sys = SystemTime::now();
        let now_instant = Instant::now();
        let utc_secs = now_sys
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let (offset_secs, tz_abbr) = local_tz_info();

        Self {
            pgm_input: AtomicUsize::new(pgm_input),
            pvw_input: AtomicUsize::new(pvw_input),
            num_inputs,
            ftb_active: AtomicBool::new(false),
            dsk_enabled: (0..num_dsk_inputs)
                .map(|_| AtomicBool::new(false))
                .collect(),
            num_dsk_inputs,
            layout,
            labels,
            instant_base: now_instant,
            base_utc_secs: utc_secs,
            tz_offset_secs: AtomicI64::new(offset_secs),
            tz_abbr_packed: AtomicU64::new(pack_tz_abbr(&tz_abbr)),
            tz_next_refresh: AtomicU64::new(60),
        }
    }

    /// Get local wall-clock time as (hours, minutes, seconds) and timezone abbreviation.
    /// Uses Instant::now() (vDSO fast path) with a cached offset that refreshes every 60s.
    fn wall_clock_hms(&self) -> (u32, u32, u32) {
        let elapsed_secs = self.instant_base.elapsed().as_secs();

        // Refresh timezone info periodically (handles DST transitions)
        let next_refresh = self.tz_next_refresh.load(Ordering::Relaxed);
        if elapsed_secs >= next_refresh {
            let (offset, abbr) = local_tz_info();
            self.tz_offset_secs.store(offset, Ordering::Relaxed);
            self.tz_abbr_packed
                .store(pack_tz_abbr(&abbr), Ordering::Relaxed);
            self.tz_next_refresh
                .store(elapsed_secs + 60, Ordering::Relaxed);
        }

        let offset = self.tz_offset_secs.load(Ordering::Relaxed);
        let utc_secs = self.base_utc_secs + elapsed_secs;
        let local_secs = (utc_secs as i64 + offset) as u64;
        let secs_of_day = local_secs % 86400;
        let h = (secs_of_day / 3600) as u32;
        let m = ((secs_of_day % 3600) / 60) as u32;
        let s = (secs_of_day % 60) as u32;
        (h, m, s)
    }

    /// Unpack the cached timezone abbreviation into a stack buffer.
    /// Returns the number of valid bytes written.
    fn tz_abbr_bytes(&self, out: &mut [u8; 7]) -> usize {
        let packed = self.tz_abbr_packed.load(Ordering::Relaxed);
        let len = ((packed >> 56) & 0x7F) as usize;
        let bytes = packed.to_le_bytes();
        let n = len.min(7);
        out[..n].copy_from_slice(&bytes[..n]);
        n
    }
}

/// Get local timezone offset in seconds east of UTC and the timezone abbreviation.
fn local_tz_info() -> (i64, String) {
    let now = chrono::Local::now();
    let offset_secs = now.offset().local_minus_utc() as i64;
    let abbr = now.format("%Z").to_string();
    (offset_secs, abbr)
}

/// Pack a timezone abbreviation (up to 7 ASCII bytes) into a u64.
/// Layout: bits 63..56 = length, bits 55..0 = bytes in little-endian order.
fn pack_tz_abbr(abbr: &str) -> u64 {
    let bytes = abbr.as_bytes();
    let len = bytes.len().min(7);
    let mut le = [0u8; 8];
    le[..len].copy_from_slice(&bytes[..len]);
    let val = u64::from_le_bytes(le);
    val | ((len as u64) << 56)
}

const PVW_R: f64 = 0.0;
const PVW_G: f64 = 0.8;
const PVW_B: f64 = 0.0;

const PGM_R: f64 = 0.9;
const PGM_G: f64 = 0.0;
const PGM_B: f64 = 0.0;

const GRAY: f64 = 0.5;

/// Helper to get text extents, returning (width, height) with a fallback.
fn text_size(cr: &cairo::Context, text: &str) -> (f64, f64) {
    match cr.text_extents(text) {
        Ok(ext) => (ext.width(), ext.height()),
        Err(_) => (text.len() as f64 * 8.0, 12.0), // rough fallback
    }
}

/// Draw a center-aligned text label with a filled background rectangle.
/// `cx` is the horizontal center, `y` is the text baseline.
#[allow(clippy::too_many_arguments)]
fn draw_label_centered(
    cr: &cairo::Context,
    text: &str,
    cx: f64,
    y: f64,
    bg_r: f64,
    bg_g: f64,
    bg_b: f64,
    bg_a: f64,
    pad_x: f64,
    pad_y: f64,
) {
    let (tw, th) = text_size(cr, text);
    let x = cx - tw / 2.0;
    cr.set_source_rgba(bg_r, bg_g, bg_b, bg_a);
    cr.rectangle(
        x - pad_x,
        y - th - pad_y,
        tw + pad_x * 2.0,
        th + pad_y * 2.0,
    );
    let _ = cr.fill();
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.move_to(x, y);
    let _ = cr.show_text(text);
}

/// Draw the multiview overlay.
///
/// Called from the cairooverlay "draw" signal on the streaming thread.
pub fn draw_overlay(state: &VisionMixerOverlayState, cr: &cairo::Context) {
    let pgm = state.pgm_input.load(Ordering::Relaxed);
    let pvw = state.pvw_input.load(Ordering::Relaxed);
    let layout = &state.layout;

    // --- PVW large border ---
    cr.set_source_rgb(PVW_R, PVW_G, PVW_B);
    cr.set_line_width(layout.pvw_border_width);
    let r = &layout.pvw_rect;
    cr.rectangle(r.x, r.y, r.w, r.h);
    let _ = cr.stroke();

    // --- PGM large border ---
    cr.set_source_rgb(PGM_R, PGM_G, PGM_B);
    cr.set_line_width(layout.pgm_border_width);
    let r = &layout.pgm_rect;
    cr.rectangle(r.x, r.y, r.w, r.h);
    let _ = cr.stroke();

    // --- Thumbnail borders (drawn around full slot including label area) ---
    for i in 0..layout.num_inputs.min(layout.thumbnail_slot_rects.len()) {
        let r = &layout.thumbnail_slot_rects[i];
        if i == pgm {
            cr.set_source_rgb(PGM_R, PGM_G, PGM_B);
            cr.set_line_width(layout.thumb_border_width);
        } else if i == pvw {
            cr.set_source_rgb(PVW_R, PVW_G, PVW_B);
            cr.set_line_width(layout.thumb_border_width);
        } else {
            cr.set_source_rgb(GRAY, GRAY, GRAY);
            cr.set_line_width(1.0);
        }
        cr.rectangle(r.x, r.y, r.w, r.h);
        let _ = cr.stroke();
    }

    // --- Input labels on thumbnails ---
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(layout.label_font_size);

    for i in 0..layout.num_inputs.min(layout.label_positions.len()) {
        let pos = &layout.label_positions[i];
        draw_label_centered(
            cr,
            &state.labels[i],
            pos.x,
            pos.y,
            0.0,
            0.0,
            0.0,
            0.6,
            2.0,
            2.0,
        );
    }

    // --- PVW / PGM header labels ---
    cr.set_font_size(layout.header_font_size);

    draw_label_centered(
        cr,
        "PVW",
        layout.pvw_label_pos.x,
        layout.pvw_label_pos.y,
        PVW_R,
        PVW_G,
        PVW_B,
        0.7,
        4.0,
        2.0,
    );
    draw_label_centered(
        cr,
        "PGM",
        layout.pgm_label_pos.x,
        layout.pgm_label_pos.y,
        PGM_R,
        PGM_G,
        PGM_B,
        0.7,
        4.0,
        2.0,
    );

    // --- Clock ---
    let (h, m, s) = state.wall_clock_hms();
    // "HH:MM:SS ABCD" — 8 time chars + space + up to 7 tz chars = 16 max
    let mut buf = [b' '; 16];
    buf[0] = b'0' + (h / 10) as u8;
    buf[1] = b'0' + (h % 10) as u8;
    buf[2] = b':';
    buf[3] = b'0' + (m / 10) as u8;
    buf[4] = b'0' + (m % 10) as u8;
    buf[5] = b':';
    buf[6] = b'0' + (s / 10) as u8;
    buf[7] = b'0' + (s % 10) as u8;
    let mut tz_buf = [0u8; 7];
    let tz_len = state.tz_abbr_bytes(&mut tz_buf);
    buf[9..9 + tz_len].copy_from_slice(&tz_buf[..tz_len]);
    let total_len = 9 + tz_len;
    // SAFETY: buf contains only ASCII digits, colons, spaces, and ASCII tz abbreviation
    let clock_str = unsafe { std::str::from_utf8_unchecked(&buf[..total_len]) };

    cr.set_font_size(layout.header_font_size * 0.8);
    let clock_cx = layout.canvas_width / 2.0;
    let clock_y = layout.header_font_size * 1.2;

    draw_label_centered(
        cr, clock_str, clock_cx, clock_y, 0.0, 0.0, 0.0, 0.7, 8.0, 4.0,
    );

    // --- FTB indicator ---
    if state.ftb_active.load(Ordering::Relaxed) {
        let r = &layout.pgm_rect;
        let ftb_cx = r.x + r.w / 2.0;
        let ftb_cy = r.y + r.h / 2.0;
        cr.set_font_size(layout.header_font_size * 2.0);
        draw_label_centered(cr, "FTB", ftb_cx, ftb_cy, 0.8, 0.0, 0.0, 0.8, 12.0, 6.0);
    }
}
