//! Audio analyzer visualization widgets (waveform oscilloscope and vectorscope).

use crate::meter::BlockDataKey;
use base64::{engine::general_purpose::STANDARD, Engine};
use egui::{Color32, Rect, Stroke, Ui, Vec2};
use instant::Instant;
use std::collections::HashMap;
use std::time::Duration;
use strom_types::FlowId;

/// Time-to-live for analyzer data before it's considered stale.
const ANALYZER_DATA_TTL: Duration = Duration::from_millis(1000);

/// Waveform color for L channel (teal).
const COLOR_L: Color32 = Color32::from_rgb(0, 180, 180);
/// Waveform color for R channel (orange).
const COLOR_R: Color32 = Color32::from_rgb(220, 140, 40);
/// Vectorscope dot color (green, classic scope look).
const COLOR_VECTOR: Color32 = Color32::from_rgb(100, 200, 100);
/// Reference line color.
const COLOR_REF: Color32 = Color32::from_rgb(60, 60, 60);

/// Audio analyzer data for a specific element.
#[derive(Debug, Clone)]
pub struct AudioAnalyzerData {
    /// Waveform min values per column for L channel (-1.0..1.0)
    pub waveform_l_min: Vec<f32>,
    /// Waveform max values per column for L channel
    pub waveform_l_max: Vec<f32>,
    /// Waveform min values per column for R channel
    pub waveform_r_min: Vec<f32>,
    /// Waveform max values per column for R channel
    pub waveform_r_max: Vec<f32>,
    /// Vectorscope L channel samples (-1.0..1.0)
    pub vectorscope_l: Vec<f32>,
    /// Vectorscope R channel samples (-1.0..1.0)
    pub vectorscope_r: Vec<f32>,
}

impl AudioAnalyzerData {
    /// Decode base64-encoded i8 samples and normalize to -1.0..1.0.
    pub fn from_base64(
        waveform_l_min: &str,
        waveform_l_max: &str,
        waveform_r_min: &str,
        waveform_r_max: &str,
        vectorscope_l: &str,
        vectorscope_r: &str,
    ) -> Self {
        Self {
            waveform_l_min: decode_and_normalize(waveform_l_min),
            waveform_l_max: decode_and_normalize(waveform_l_max),
            waveform_r_min: decode_and_normalize(waveform_r_min),
            waveform_r_max: decode_and_normalize(waveform_r_max),
            vectorscope_l: decode_and_normalize(vectorscope_l),
            vectorscope_r: decode_and_normalize(vectorscope_r),
        }
    }
}

/// Decode a base64 string of i8 bytes and normalize each to -1.0..1.0.
fn decode_and_normalize(b64: &str) -> Vec<f32> {
    let bytes = STANDARD.decode(b64).unwrap_or_default();
    bytes.iter().map(|&b| (b as i8) as f32 / 128.0).collect()
}

/// Analyzer data with timestamp for TTL tracking.
#[derive(Debug, Clone)]
struct TimestampedData {
    data: AudioAnalyzerData,
    updated_at: Instant,
}

/// Storage for all audio analyzer data in the application.
#[derive(Debug, Clone, Default)]
pub struct AudioAnalyzerDataStore {
    data: HashMap<BlockDataKey, TimestampedData>,
}

impl AudioAnalyzerDataStore {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    /// Update analyzer data for a specific element.
    pub fn update(&mut self, flow_id: FlowId, element_id: String, data: AudioAnalyzerData) {
        let key = BlockDataKey {
            flow_id,
            element_id,
        };
        self.data.insert(
            key,
            TimestampedData {
                data,
                updated_at: Instant::now(),
            },
        );
    }

    /// Get analyzer data for a specific element.
    /// Returns None if the data is stale (older than TTL).
    pub fn get(&self, flow_id: &FlowId, element_id: &str) -> Option<&AudioAnalyzerData> {
        let key = BlockDataKey {
            flow_id: *flow_id,
            element_id: element_id.to_string(),
        };
        self.data.get(&key).and_then(|timestamped| {
            if timestamped.updated_at.elapsed() < ANALYZER_DATA_TTL {
                Some(&timestamped.data)
            } else {
                None
            }
        })
    }
}

/// Calculate the height needed for a compact audio analyzer display.
pub fn calculate_compact_height() -> f32 {
    // Waveform (two channels stacked) + some padding
    80.0
}

/// Render a compact audio analyzer (waveform + vectorscope side by side).
pub fn show_compact(ui: &mut Ui, data: &AudioAnalyzerData) {
    let available = ui.available_size();
    let total_width = available.x.max(120.0);
    let total_height = available.y.max(60.0);

    // Allocate space for vectorscope (square, using height as size)
    let scope_size = total_height.min(total_width * 0.35);
    let waveform_width = (total_width - scope_size - 4.0).max(60.0);

    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(total_width, total_height), egui::Sense::hover());

    let painter = ui.painter_at(rect);

    // Waveform area (left side)
    let waveform_rect = Rect::from_min_size(rect.min, Vec2::new(waveform_width, total_height));
    draw_waveform(&painter, waveform_rect, data);

    // Vectorscope area (right side, square)
    let scope_rect = Rect::from_min_size(
        egui::pos2(
            rect.min.x + waveform_width + 4.0,
            rect.min.y + (total_height - scope_size) / 2.0,
        ),
        Vec2::new(scope_size, scope_size),
    );
    draw_vectorscope(&painter, scope_rect, data);
}

/// Draw the waveform oscilloscope display.
fn draw_waveform(painter: &egui::Painter, rect: Rect, data: &AudioAnalyzerData) {
    // Background
    painter.rect(
        rect,
        1.0,
        Color32::from_rgb(15, 15, 20),
        Stroke::new(1.0, Color32::from_gray(50)),
        egui::epaint::StrokeKind::Inside,
    );

    let half_h = rect.height() / 2.0;

    // L channel: top half, R channel: bottom half
    let l_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), half_h));
    let r_rect = Rect::from_min_size(
        egui::pos2(rect.min.x, rect.min.y + half_h),
        Vec2::new(rect.width(), half_h),
    );

    // Center lines (zero crossing)
    let l_center_y = l_rect.center().y;
    let r_center_y = r_rect.center().y;
    painter.line_segment(
        [
            egui::pos2(rect.min.x, l_center_y),
            egui::pos2(rect.max.x, l_center_y),
        ],
        Stroke::new(0.5, COLOR_REF),
    );
    painter.line_segment(
        [
            egui::pos2(rect.min.x, r_center_y),
            egui::pos2(rect.max.x, r_center_y),
        ],
        Stroke::new(0.5, COLOR_REF),
    );

    // Divider between L and R
    painter.line_segment(
        [
            egui::pos2(rect.min.x, rect.min.y + half_h),
            egui::pos2(rect.max.x, rect.min.y + half_h),
        ],
        Stroke::new(0.5, COLOR_REF),
    );

    // Draw L waveform
    draw_channel_waveform(
        painter,
        l_rect,
        &data.waveform_l_min,
        &data.waveform_l_max,
        COLOR_L,
    );

    // Draw R waveform
    draw_channel_waveform(
        painter,
        r_rect,
        &data.waveform_r_min,
        &data.waveform_r_max,
        COLOR_R,
    );
}

/// Draw a single channel's waveform in the given rect.
fn draw_channel_waveform(
    painter: &egui::Painter,
    rect: Rect,
    mins: &[f32],
    maxs: &[f32],
    color: Color32,
) {
    let num_columns = mins.len();
    if num_columns == 0 {
        return;
    }

    let center_y = rect.center().y;
    let half_h = rect.height() / 2.0 * 0.9; // 90% to leave a small margin
    let col_width = rect.width() / num_columns as f32;

    for (i, (&min_val, &max_val)) in mins.iter().zip(maxs.iter()).enumerate() {
        let x = rect.min.x + (i as f32 + 0.5) * col_width;

        // Map -1.0..1.0 to pixel Y (inverted: -1.0 is bottom, 1.0 is top)
        let y_min = center_y - max_val * half_h; // max_val maps to higher on screen (lower Y)
        let y_max = center_y - min_val * half_h; // min_val maps to lower on screen (higher Y)

        painter.line_segment(
            [egui::pos2(x, y_min), egui::pos2(x, y_max)],
            Stroke::new(1.0, color),
        );
    }
}

/// Draw the vectorscope (Lissajous) display.
fn draw_vectorscope(painter: &egui::Painter, rect: Rect, data: &AudioAnalyzerData) {
    // Background
    painter.rect(
        rect,
        1.0,
        Color32::from_rgb(15, 15, 20),
        Stroke::new(1.0, Color32::from_gray(50)),
        egui::epaint::StrokeKind::Inside,
    );

    let center = rect.center();
    let half_size = rect.width().min(rect.height()) / 2.0 * 0.9;

    // Reference lines: crosshairs
    painter.line_segment(
        [
            egui::pos2(rect.min.x, center.y),
            egui::pos2(rect.max.x, center.y),
        ],
        Stroke::new(0.5, COLOR_REF),
    );
    painter.line_segment(
        [
            egui::pos2(center.x, rect.min.y),
            egui::pos2(center.x, rect.max.y),
        ],
        Stroke::new(0.5, COLOR_REF),
    );

    // Reference lines: +45deg (mono) and -45deg (phase inversion)
    let diag_len = half_size * 0.95;
    painter.line_segment(
        [
            egui::pos2(center.x - diag_len, center.y - diag_len),
            egui::pos2(center.x + diag_len, center.y + diag_len),
        ],
        Stroke::new(0.5, Color32::from_rgb(40, 40, 50)),
    );
    painter.line_segment(
        [
            egui::pos2(center.x - diag_len, center.y + diag_len),
            egui::pos2(center.x + diag_len, center.y - diag_len),
        ],
        Stroke::new(0.5, Color32::from_rgb(40, 40, 50)),
    );

    // Draw points: X = L, Y = R (standard vectorscope convention)
    for (&l, &r) in data.vectorscope_l.iter().zip(data.vectorscope_r.iter()) {
        let x = center.x + l * half_size;
        let y = center.y - r * half_size; // Invert Y for screen coords

        painter.rect_filled(
            Rect::from_center_size(egui::pos2(x, y), Vec2::splat(1.5)),
            0.0,
            COLOR_VECTOR,
        );
    }
}
