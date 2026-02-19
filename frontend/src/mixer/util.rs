//! Utility functions for dB/linear conversions and formatting.

use egui::Color32;

/// Format a linear fader value as dB string.
pub(super) fn format_db(linear: f32) -> String {
    if !linear.is_finite() || linear <= 0.001 {
        "-inf dB".to_string()
    } else {
        let db = 20.0 * linear.log10();
        if db <= -59.0 {
            "-inf dB".to_string()
        } else {
            format!("{:.1} dB", db)
        }
    }
}

/// Format a pan value as string.
pub(super) fn format_pan(pan: f32) -> String {
    if pan < -0.01 {
        format!("L{:.0}", (-pan * 100.0))
    } else if pan > 0.01 {
        format!("R{:.0}", (pan * 100.0))
    } else {
        "C".to_string()
    }
}

/// Map a dB value to a y-coordinate within a vertical range.
/// All faders, meters, and scales share this mapping for alignment.
/// Range: -60 dB at bottom (y_max - 5px) to +6 dB at top (y_min + 5px).
pub(super) fn db_to_y(db: f32, y_min: f32, y_max: f32) -> f32 {
    let db = if db.is_finite() { db } else { -60.0 };
    let normalized = ((db - (-60.0)) / 66.0).clamp(0.0, 1.0);
    let margin = 5.0;
    let usable = (y_max - y_min) - margin * 2.0;
    y_max - margin - normalized * usable
}

/// Convert dB to linear level (0.0-1.0).
pub(super) fn db_to_level(db: f64) -> f32 {
    if !db.is_finite() {
        return 0.0;
    }
    let min_db = -60.0;
    let max_db = 6.0;
    ((db - min_db) / (max_db - min_db)).clamp(0.0, 1.0) as f32
}

/// Get color for a level value.
pub(super) fn level_to_color(level: f32) -> Color32 {
    if level < 0.7 {
        Color32::from_rgb(0, 200, 0) // Green
    } else if level < 0.85 {
        Color32::from_rgb(255, 220, 0) // Yellow
    } else if level < 0.9 {
        Color32::from_rgb(255, 165, 0) // Orange
    } else {
        Color32::from_rgb(255, 0, 0) // Red
    }
}

/// Convert dB to linear scale (f64).
pub(super) fn db_to_linear_f64(db: f64) -> f64 {
    if !db.is_finite() {
        return 0.0;
    }
    10.0_f64.powf(db / 20.0)
}

/// Convert dB to linear scale (f32).
pub(super) fn db_to_linear_f32(db: f32) -> f32 {
    if !db.is_finite() || db <= -60.0 {
        0.0
    } else {
        10.0_f32.powf(db / 20.0)
    }
}

/// Convert linear to dB scale.
pub(super) fn linear_to_db(linear: f64) -> f64 {
    if !linear.is_finite() || linear <= 0.0001 {
        -60.0
    } else {
        20.0 * linear.log10()
    }
}

/// Convert a linear level (0.0-2.0) to a knob arc position (0.0-1.0).
///
/// dB-scaled: first half of arc = -60..0 dB, second half = 0..+6 dB.
/// This puts unity (0 dB, linear 1.0) at the center of the arc (12 o'clock).
pub(super) fn knob_linear_to_normalized(linear: f32) -> f32 {
    if !linear.is_finite() || linear <= 0.001 {
        return 0.0;
    }
    let db = 20.0 * linear.log10();
    if db <= -60.0 {
        0.0
    } else if db <= 0.0 {
        // -60..0 dB maps to 0.0..0.5
        0.5 * (db + 60.0) / 60.0
    } else {
        // 0..+6 dB maps to 0.5..1.0
        (0.5 + 0.5 * db / 6.0).min(1.0)
    }
}
