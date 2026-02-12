//! Mixer editor - fullscreen audio mixer view.
//!
//! Provides an interactive mixer console similar to digital mixers like Behringer X32:
//! - Per-channel faders, pan controls, mute buttons
//! - Real-time metering
//! - Keyboard shortcuts for quick mixing
//!
//! Per-channel gate, compressor, 4-band EQ, aux sends, groups, PFL

use egui::{Color32, Context, CornerRadius, Rect, Response, Sense, Stroke, Ui, Vec2};
use std::collections::HashMap;
use strom_types::{FlowId, PropertyValue};

use crate::api::ApiClient;
use crate::meter::{MeterData, MeterDataStore};

/// Default fader value (0 dB unity)
const DEFAULT_FADER: f32 = 1.0;

/// Maximum number of aux buses
const MAX_AUX_BUSES: usize = 4;
/// Maximum number of groups
const MAX_GROUPS: usize = 4;

// ── Layout constants ─────────────────────────────────────────────────
/// Gap between strips
const STRIP_GAP: f32 = 2.0;
/// Inner margin inside each strip frame
const STRIP_MARGIN: f32 = 3.0;
/// Preferred knob diameter
const KNOB_SIZE: f32 = 22.0;
/// Small button height (G/C/E, routing)
const SMALL_BTN_H: f32 = 18.0;
/// Standard button height (mute, PFL)
const BTN_H: f32 = 20.0;
/// LCD display height
const LCD_H: f32 = 16.0;
/// Pan knob diameter
const PAN_KNOB_SIZE: f32 = 24.0;
/// Height of the fader + meter area
const FADER_HEIGHT: f32 = 220.0;
/// Minimum usable inner width (for 0 aux sends)
const MIN_STRIP_INNER: f32 = 42.0;
/// Height of bus master faders (shorter than channel faders)
const BUS_FADER_HEIGHT: f32 = 120.0;
/// Fixed inner width for bus master strips
const BUS_STRIP_INNER: f32 = 52.0;
/// Minimum height for the bus master row
const BUS_ROW_MIN_HEIGHT: f32 = 200.0;

/// A single channel strip in the mixer.
#[derive(Debug, Clone)]
struct ChannelStrip {
    /// Channel number (1-indexed)
    channel_num: usize,
    /// Channel label
    label: String,
    /// Input gain (dB)
    gain: f32,
    /// Pan position (-1.0 to 1.0)
    pan: f32,
    /// Fader level (0.0 to 2.0)
    fader: f32,
    /// Mute state
    mute: bool,
    /// PFL (Pre-Fader Listen) state
    pfl: bool,
    /// Route to main mix
    to_main: bool,
    /// Route to groups (up to 4)
    to_grp: [bool; MAX_GROUPS],
    /// Aux send levels (up to 4 aux buses)
    aux_sends: [f32; MAX_AUX_BUSES],
    /// Aux send pre-fader mode (true=pre, false=post)
    aux_pre: [bool; MAX_AUX_BUSES],
    /// HPF enabled
    hpf_enabled: bool,
    /// HPF cutoff frequency (Hz)
    hpf_freq: f32,
    /// Gate enabled
    gate_enabled: bool,
    /// Gate threshold (dB)
    gate_threshold: f32,
    /// Gate attack (ms)
    gate_attack: f32,
    /// Gate release (ms)
    gate_release: f32,
    /// Gate range (dB)
    gate_range: f32,
    /// Compressor enabled
    comp_enabled: bool,
    /// Compressor threshold (dB)
    comp_threshold: f32,
    /// Compressor ratio
    comp_ratio: f32,
    /// Compressor attack (ms)
    comp_attack: f32,
    /// Compressor release (ms)
    comp_release: f32,
    /// Compressor makeup gain (dB)
    comp_makeup: f32,
    /// Compressor knee (dB)
    comp_knee: f32,
    /// EQ enabled
    eq_enabled: bool,
    /// EQ bands: (freq, gain_db, q) for 4 bands
    eq_bands: [(f32, f32, f32); 4],
    /// Pending API update
    pending_update: bool,
}

/// Group strip state.
#[derive(Debug, Clone)]
struct GroupStrip {
    /// Group index (0-based)
    index: usize,
    /// Fader level (0.0 to 2.0)
    fader: f32,
    /// Mute state
    mute: bool,
}

/// Aux bus master state.
#[derive(Debug, Clone)]
struct AuxMaster {
    /// Aux index (0-based)
    index: usize,
    /// Master fader level
    fader: f32,
    /// Mute state
    mute: bool,
}

impl ChannelStrip {
    fn new(channel_num: usize) -> Self {
        Self {
            channel_num,
            label: format!("Ch {}", channel_num),
            gain: 0.0,
            pan: 0.0,
            fader: DEFAULT_FADER,
            mute: false,
            pfl: false,
            to_main: true,
            to_grp: [false; MAX_GROUPS],
            aux_sends: [0.0; MAX_AUX_BUSES],
            aux_pre: [true, true, false, false], // aux 1-2 pre, 3-4 post
            hpf_enabled: false,
            hpf_freq: 80.0,
            gate_enabled: false,
            gate_threshold: -40.0,
            gate_attack: 5.0,
            gate_release: 100.0,
            gate_range: -80.0,
            comp_enabled: false,
            comp_threshold: -20.0,
            comp_ratio: 4.0,
            comp_attack: 10.0,
            comp_release: 100.0,
            comp_makeup: 0.0,
            comp_knee: -6.0,
            eq_enabled: false,
            eq_bands: [
                (80.0, 0.0, 1.0),   // Low
                (400.0, 0.0, 1.0),  // Low-mid
                (2000.0, 0.0, 1.0), // High-mid
                (8000.0, 0.0, 1.0), // High
            ],
            pending_update: false,
        }
    }
}

impl GroupStrip {
    fn new(index: usize) -> Self {
        Self {
            index,
            fader: 1.0,
            mute: false,
        }
    }
}

impl AuxMaster {
    fn new(index: usize) -> Self {
        Self {
            index,
            fader: 1.0,
            mute: false,
        }
    }
}

/// What control is currently being adjusted (for value display).
#[derive(Debug, Clone, PartialEq)]
enum ActiveControl {
    None,
    Pan(usize),            // Channel index
    Fader(usize),          // Channel index
    AuxSend(usize, usize), // (Channel index, Aux index)
    GroupFader(usize),     // Group index
    AuxMasterFader(usize), // Aux master index
    MainFader,
}

/// What is currently selected in the mixer.
#[derive(Debug, Clone, PartialEq)]
enum Selection {
    Channel(usize),
    Main,
}

/// Mixer editor state.
pub struct MixerEditor {
    /// Flow ID
    flow_id: FlowId,
    /// Block ID (e.g., "b0")
    block_id: String,

    /// Number of channels
    num_channels: usize,
    /// Number of aux buses
    num_aux_buses: usize,
    /// Number of groups
    num_groups: usize,

    /// Channel strips
    channels: Vec<ChannelStrip>,
    /// Group strips
    groups: Vec<GroupStrip>,
    /// Aux masters
    aux_masters: Vec<AuxMaster>,

    /// Currently selected strip (channel or bus)
    selection: Option<Selection>,
    /// Currently active control (for value display)
    active_control: ActiveControl,

    /// Main fader level
    main_fader: f32,
    /// Main mute
    main_mute: bool,
    /// Main bus compressor enabled
    main_comp_enabled: bool,
    /// Main bus compressor threshold (dB)
    main_comp_threshold: f32,
    /// Main bus compressor ratio
    main_comp_ratio: f32,
    /// Main bus compressor attack (ms)
    main_comp_attack: f32,
    /// Main bus compressor release (ms)
    main_comp_release: f32,
    /// Main bus compressor makeup gain (dB)
    main_comp_makeup: f32,
    /// Main bus compressor knee (dB)
    main_comp_knee: f32,
    /// Main bus EQ enabled
    main_eq_enabled: bool,
    /// Main bus EQ bands: (freq, gain_db, q) for 4 bands
    main_eq_bands: [(f32, f32, f32); 4],
    /// Main bus limiter enabled
    main_limiter_enabled: bool,
    /// Main bus limiter threshold (dB)
    main_limiter_threshold: f32,

    /// API client
    api: ApiClient,

    /// Status message
    status: String,
    /// Error message
    error: Option<String>,

    /// Live updates enabled
    live_updates: bool,
    /// Last update time (for throttling)
    last_update: instant::Instant,
    /// Save requested (checked by app to persist properties)
    save_requested: bool,
    /// True after reset — next save writes only structural properties
    is_reset: bool,
}

impl MixerEditor {
    /// Create a new mixer editor.
    pub fn new(flow_id: FlowId, block_id: String, num_channels: usize, api: ApiClient) -> Self {
        let channels = (1..=num_channels).map(ChannelStrip::new).collect();

        Self {
            flow_id,
            block_id,
            num_channels,
            num_aux_buses: 0,
            num_groups: 0,
            channels,
            groups: Vec::new(),
            aux_masters: Vec::new(),
            selection: None,
            active_control: ActiveControl::None,
            main_fader: 1.0,
            main_mute: false,
            main_comp_enabled: false,
            main_comp_threshold: -20.0,
            main_comp_ratio: 4.0,
            main_comp_attack: 10.0,
            main_comp_release: 100.0,
            main_comp_makeup: 0.0,
            main_comp_knee: -6.0,
            main_eq_enabled: false,
            main_eq_bands: [
                (80.0, 0.0, 1.0),
                (400.0, 0.0, 1.0),
                (2000.0, 0.0, 1.0),
                (8000.0, 0.0, 1.0),
            ],
            main_limiter_enabled: false,
            main_limiter_threshold: -3.0,
            api,
            status: String::new(),
            error: None,
            live_updates: true,
            last_update: instant::Instant::now(),
            save_requested: false,
            is_reset: false,
        }
    }

    /// Compute the usable inner width of a strip based on number of aux buses.
    /// The aux knob row is typically the widest element.
    fn strip_inner(&self) -> f32 {
        if self.num_aux_buses == 0 {
            return MIN_STRIP_INNER;
        }
        let knob_row =
            self.num_aux_buses as f32 * KNOB_SIZE + (self.num_aux_buses as f32 - 1.0) * 2.0;
        knob_row.max(MIN_STRIP_INNER)
    }

    /// Total strip width including margins.
    fn strip_width(&self) -> f32 {
        self.strip_inner() + STRIP_MARGIN * 2.0
    }

    /// Load channel values from block properties.
    pub fn load_from_properties(&mut self, properties: &HashMap<String, PropertyValue>) {
        // Load main fader
        if let Some(PropertyValue::Float(f)) = properties.get("main_fader") {
            self.main_fader = *f as f32;
        }

        // Load main bus processing
        if let Some(PropertyValue::Bool(b)) = properties.get("main_comp_enabled") {
            self.main_comp_enabled = *b;
        }
        if let Some(PropertyValue::Float(f)) = properties.get("main_comp_threshold") {
            self.main_comp_threshold = *f as f32;
        }
        if let Some(PropertyValue::Float(f)) = properties.get("main_comp_ratio") {
            self.main_comp_ratio = *f as f32;
        }
        if let Some(PropertyValue::Float(f)) = properties.get("main_comp_attack") {
            self.main_comp_attack = *f as f32;
        }
        if let Some(PropertyValue::Float(f)) = properties.get("main_comp_release") {
            self.main_comp_release = *f as f32;
        }
        if let Some(PropertyValue::Float(f)) = properties.get("main_comp_makeup") {
            self.main_comp_makeup = *f as f32;
        }
        if let Some(PropertyValue::Float(f)) = properties.get("main_comp_knee") {
            self.main_comp_knee = *f as f32;
        }
        if let Some(PropertyValue::Bool(b)) = properties.get("main_eq_enabled") {
            self.main_eq_enabled = *b;
        }
        for band in 0..4 {
            let band_num = band + 1;
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("main_eq{}_freq", band_num))
            {
                self.main_eq_bands[band].0 = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("main_eq{}_gain", band_num))
            {
                self.main_eq_bands[band].1 = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) = properties.get(&format!("main_eq{}_q", band_num))
            {
                self.main_eq_bands[band].2 = *f as f32;
            }
        }
        if let Some(PropertyValue::Bool(b)) = properties.get("main_limiter_enabled") {
            self.main_limiter_enabled = *b;
        }
        if let Some(PropertyValue::Float(f)) = properties.get("main_limiter_threshold") {
            self.main_limiter_threshold = *f as f32;
        }

        // Load number of aux buses
        self.num_aux_buses = properties
            .get("num_aux_buses")
            .and_then(|v| match v {
                PropertyValue::Int(i) => Some(*i as usize),
                PropertyValue::String(s) => s.parse().ok(),
                _ => None,
            })
            .unwrap_or(0)
            .min(MAX_AUX_BUSES);

        // Load number of groups
        self.num_groups = properties
            .get("num_groups")
            .and_then(|v| match v {
                PropertyValue::Int(i) => Some(*i as usize),
                PropertyValue::String(s) => s.parse().ok(),
                _ => None,
            })
            .unwrap_or(0)
            .min(MAX_GROUPS);

        // Initialize groups
        self.groups = (0..self.num_groups)
            .map(|i| {
                let mut sg = GroupStrip::new(i);
                if let Some(PropertyValue::Float(f)) =
                    properties.get(&format!("group{}_fader", i + 1))
                {
                    sg.fader = *f as f32;
                }
                if let Some(PropertyValue::Bool(b)) =
                    properties.get(&format!("group{}_mute", i + 1))
                {
                    sg.mute = *b;
                }
                sg
            })
            .collect();

        // Initialize aux masters
        self.aux_masters = (0..self.num_aux_buses)
            .map(|i| {
                let mut aux = AuxMaster::new(i);
                if let Some(PropertyValue::Float(f)) =
                    properties.get(&format!("aux{}_fader", i + 1))
                {
                    aux.fader = *f as f32;
                }
                if let Some(PropertyValue::Bool(b)) = properties.get(&format!("aux{}_mute", i + 1))
                {
                    aux.mute = *b;
                }
                aux
            })
            .collect();

        // Load per-channel properties
        for ch in &mut self.channels {
            let ch_num = ch.channel_num;

            // Label
            if let Some(PropertyValue::String(s)) = properties.get(&format!("ch{}_label", ch_num)) {
                ch.label = s.clone();
            }
            // Input gain
            if let Some(PropertyValue::Float(f)) = properties.get(&format!("ch{}_gain", ch_num)) {
                ch.gain = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) = properties.get(&format!("ch{}_pan", ch_num)) {
                ch.pan = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) = properties.get(&format!("ch{}_fader", ch_num)) {
                ch.fader = *f as f32;
            }
            if let Some(PropertyValue::Bool(b)) = properties.get(&format!("ch{}_mute", ch_num)) {
                ch.mute = *b;
            }
            if let Some(PropertyValue::Bool(b)) = properties.get(&format!("ch{}_pfl", ch_num)) {
                ch.pfl = *b;
            }
            // Routing to main
            if let Some(PropertyValue::Bool(b)) = properties.get(&format!("ch{}_to_main", ch_num)) {
                ch.to_main = *b;
            }
            // Routing to groups
            for sg in 0..MAX_GROUPS {
                if let Some(PropertyValue::Bool(b)) =
                    properties.get(&format!("ch{}_to_grp{}", ch_num, sg + 1))
                {
                    ch.to_grp[sg] = *b;
                }
            }
            // Aux send levels and pre/post
            for aux in 0..MAX_AUX_BUSES {
                if let Some(PropertyValue::Float(f)) =
                    properties.get(&format!("ch{}_aux{}_level", ch_num, aux + 1))
                {
                    ch.aux_sends[aux] = *f as f32;
                }
                if let Some(PropertyValue::Bool(b)) =
                    properties.get(&format!("ch{}_aux{}_pre", ch_num, aux + 1))
                {
                    ch.aux_pre[aux] = *b;
                }
            }
            // HPF
            if let Some(PropertyValue::Bool(b)) =
                properties.get(&format!("ch{}_hpf_enabled", ch_num))
            {
                ch.hpf_enabled = *b;
            }
            if let Some(PropertyValue::Float(f)) = properties.get(&format!("ch{}_hpf_freq", ch_num))
            {
                ch.hpf_freq = *f as f32;
            }
            // Gate
            if let Some(PropertyValue::Bool(b)) =
                properties.get(&format!("ch{}_gate_enabled", ch_num))
            {
                ch.gate_enabled = *b;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_gate_threshold", ch_num))
            {
                ch.gate_threshold = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_gate_attack", ch_num))
            {
                ch.gate_attack = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_gate_release", ch_num))
            {
                ch.gate_release = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_gate_range", ch_num))
            {
                ch.gate_range = *f as f32;
            }
            // Compressor
            if let Some(PropertyValue::Bool(b)) =
                properties.get(&format!("ch{}_comp_enabled", ch_num))
            {
                ch.comp_enabled = *b;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_comp_threshold", ch_num))
            {
                ch.comp_threshold = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_comp_ratio", ch_num))
            {
                ch.comp_ratio = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_comp_attack", ch_num))
            {
                ch.comp_attack = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_comp_release", ch_num))
            {
                ch.comp_release = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_comp_makeup", ch_num))
            {
                ch.comp_makeup = *f as f32;
            }
            if let Some(PropertyValue::Float(f)) =
                properties.get(&format!("ch{}_comp_knee", ch_num))
            {
                ch.comp_knee = *f as f32;
            }
            // EQ
            if let Some(PropertyValue::Bool(b)) =
                properties.get(&format!("ch{}_eq_enabled", ch_num))
            {
                ch.eq_enabled = *b;
            }
            for band in 0..4 {
                let band_num = band + 1;
                if let Some(PropertyValue::Float(f)) =
                    properties.get(&format!("ch{}_eq{}_freq", ch_num, band_num))
                {
                    ch.eq_bands[band].0 = *f as f32;
                }
                if let Some(PropertyValue::Float(f)) =
                    properties.get(&format!("ch{}_eq{}_gain", ch_num, band_num))
                {
                    ch.eq_bands[band].1 = *f as f32;
                }
                if let Some(PropertyValue::Float(f)) =
                    properties.get(&format!("ch{}_eq{}_q", ch_num, band_num))
                {
                    ch.eq_bands[band].2 = *f as f32;
                }
            }
        }
    }

    /// Get the block ID.
    pub fn block_id(&self) -> &str {
        &self.block_id
    }

    /// Get the flow ID.
    pub fn flow_id(&self) -> FlowId {
        self.flow_id
    }

    /// Check if a save was requested (Ctrl+S or Save button).
    pub fn needs_save(&self) -> bool {
        self.save_requested
    }

    /// Clear the save-requested flag.
    pub fn clear_save(&mut self) {
        self.save_requested = false;
        self.is_reset = false;
    }

    /// True if this save is a reset (only structural properties should be saved).
    pub fn is_reset(&self) -> bool {
        self.is_reset
    }

    /// Collect all current mixer state as block properties.
    pub fn collect_properties(&self) -> HashMap<String, PropertyValue> {
        let mut props = HashMap::new();

        // Structural properties
        props.insert(
            "num_channels".to_string(),
            PropertyValue::Int(self.num_channels as i64),
        );
        props.insert(
            "num_aux_buses".to_string(),
            PropertyValue::Int(self.num_aux_buses as i64),
        );
        props.insert(
            "num_groups".to_string(),
            PropertyValue::Int(self.num_groups as i64),
        );

        // Main bus
        props.insert(
            "main_fader".to_string(),
            PropertyValue::Float(self.main_fader as f64),
        );
        props.insert(
            "main_comp_enabled".to_string(),
            PropertyValue::Bool(self.main_comp_enabled),
        );
        props.insert(
            "main_comp_threshold".to_string(),
            PropertyValue::Float(self.main_comp_threshold as f64),
        );
        props.insert(
            "main_comp_ratio".to_string(),
            PropertyValue::Float(self.main_comp_ratio as f64),
        );
        props.insert(
            "main_comp_attack".to_string(),
            PropertyValue::Float(self.main_comp_attack as f64),
        );
        props.insert(
            "main_comp_release".to_string(),
            PropertyValue::Float(self.main_comp_release as f64),
        );
        props.insert(
            "main_comp_makeup".to_string(),
            PropertyValue::Float(self.main_comp_makeup as f64),
        );
        props.insert(
            "main_comp_knee".to_string(),
            PropertyValue::Float(self.main_comp_knee as f64),
        );
        props.insert(
            "main_eq_enabled".to_string(),
            PropertyValue::Bool(self.main_eq_enabled),
        );
        for (band, (freq, gain, q)) in self.main_eq_bands.iter().enumerate() {
            let b = band + 1;
            props.insert(
                format!("main_eq{}_freq", b),
                PropertyValue::Float(*freq as f64),
            );
            props.insert(
                format!("main_eq{}_gain", b),
                PropertyValue::Float(*gain as f64),
            );
            props.insert(format!("main_eq{}_q", b), PropertyValue::Float(*q as f64));
        }
        props.insert(
            "main_limiter_enabled".to_string(),
            PropertyValue::Bool(self.main_limiter_enabled),
        );
        props.insert(
            "main_limiter_threshold".to_string(),
            PropertyValue::Float(self.main_limiter_threshold as f64),
        );

        // Aux masters
        for aux in &self.aux_masters {
            let n = aux.index + 1;
            props.insert(
                format!("aux{}_fader", n),
                PropertyValue::Float(aux.fader as f64),
            );
            props.insert(format!("aux{}_mute", n), PropertyValue::Bool(aux.mute));
        }

        // Groups
        for sg in &self.groups {
            let n = sg.index + 1;
            props.insert(
                format!("group{}_fader", n),
                PropertyValue::Float(sg.fader as f64),
            );
            props.insert(format!("group{}_mute", n), PropertyValue::Bool(sg.mute));
        }

        // Per-channel
        for ch in &self.channels {
            let n = ch.channel_num;
            props.insert(
                format!("ch{}_label", n),
                PropertyValue::String(ch.label.clone()),
            );
            props.insert(
                format!("ch{}_gain", n),
                PropertyValue::Float(ch.gain as f64),
            );
            props.insert(format!("ch{}_pan", n), PropertyValue::Float(ch.pan as f64));
            props.insert(
                format!("ch{}_fader", n),
                PropertyValue::Float(ch.fader as f64),
            );
            props.insert(format!("ch{}_mute", n), PropertyValue::Bool(ch.mute));
            props.insert(format!("ch{}_pfl", n), PropertyValue::Bool(ch.pfl));
            props.insert(format!("ch{}_to_main", n), PropertyValue::Bool(ch.to_main));
            for (sg, &enabled) in ch.to_grp.iter().enumerate().take(self.num_groups) {
                props.insert(
                    format!("ch{}_to_grp{}", n, sg + 1),
                    PropertyValue::Bool(enabled),
                );
            }
            for aux in 0..self.num_aux_buses {
                props.insert(
                    format!("ch{}_aux{}_level", n, aux + 1),
                    PropertyValue::Float(ch.aux_sends[aux] as f64),
                );
                props.insert(
                    format!("ch{}_aux{}_pre", n, aux + 1),
                    PropertyValue::Bool(ch.aux_pre[aux]),
                );
            }
            // HPF
            props.insert(
                format!("ch{}_hpf_enabled", n),
                PropertyValue::Bool(ch.hpf_enabled),
            );
            props.insert(
                format!("ch{}_hpf_freq", n),
                PropertyValue::Float(ch.hpf_freq as f64),
            );
            // Gate
            props.insert(
                format!("ch{}_gate_enabled", n),
                PropertyValue::Bool(ch.gate_enabled),
            );
            props.insert(
                format!("ch{}_gate_threshold", n),
                PropertyValue::Float(ch.gate_threshold as f64),
            );
            props.insert(
                format!("ch{}_gate_attack", n),
                PropertyValue::Float(ch.gate_attack as f64),
            );
            props.insert(
                format!("ch{}_gate_release", n),
                PropertyValue::Float(ch.gate_release as f64),
            );
            props.insert(
                format!("ch{}_gate_range", n),
                PropertyValue::Float(ch.gate_range as f64),
            );
            // Compressor
            props.insert(
                format!("ch{}_comp_enabled", n),
                PropertyValue::Bool(ch.comp_enabled),
            );
            props.insert(
                format!("ch{}_comp_threshold", n),
                PropertyValue::Float(ch.comp_threshold as f64),
            );
            props.insert(
                format!("ch{}_comp_ratio", n),
                PropertyValue::Float(ch.comp_ratio as f64),
            );
            props.insert(
                format!("ch{}_comp_attack", n),
                PropertyValue::Float(ch.comp_attack as f64),
            );
            props.insert(
                format!("ch{}_comp_release", n),
                PropertyValue::Float(ch.comp_release as f64),
            );
            props.insert(
                format!("ch{}_comp_makeup", n),
                PropertyValue::Float(ch.comp_makeup as f64),
            );
            props.insert(
                format!("ch{}_comp_knee", n),
                PropertyValue::Float(ch.comp_knee as f64),
            );
            // EQ
            props.insert(
                format!("ch{}_eq_enabled", n),
                PropertyValue::Bool(ch.eq_enabled),
            );
            for (band, (freq, gain, q)) in ch.eq_bands.iter().enumerate() {
                let b = band + 1;
                props.insert(
                    format!("ch{}_eq{}_freq", n, b),
                    PropertyValue::Float(*freq as f64),
                );
                props.insert(
                    format!("ch{}_eq{}_gain", n, b),
                    PropertyValue::Float(*gain as f64),
                );
                props.insert(
                    format!("ch{}_eq{}_q", n, b),
                    PropertyValue::Float(*q as f64),
                );
            }
        }

        props
    }

    /// Reset all mixer parameters to defaults.
    /// Resets in-memory state and sets `reset_properties` so the save
    /// only writes structural keys, letting backend defaults take over.
    fn reset_to_defaults(&mut self) {
        // Main bus
        self.main_fader = 1.0;
        self.main_mute = false;
        self.main_comp_enabled = false;
        self.main_comp_threshold = -20.0;
        self.main_comp_ratio = 4.0;
        self.main_comp_attack = 10.0;
        self.main_comp_release = 100.0;
        self.main_comp_makeup = 0.0;
        self.main_comp_knee = -6.0;
        self.main_eq_enabled = false;
        self.main_eq_bands = [
            (80.0, 0.0, 1.0),
            (400.0, 0.0, 1.0),
            (2000.0, 0.0, 1.0),
            (8000.0, 0.0, 1.0),
        ];
        self.main_limiter_enabled = false;
        self.main_limiter_threshold = -3.0;

        // Aux masters
        for aux in &mut self.aux_masters {
            aux.fader = 1.0;
            aux.mute = false;
        }

        // Groups
        for sg in &mut self.groups {
            sg.fader = 1.0;
            sg.mute = false;
        }

        // Channels
        for ch in &mut self.channels {
            ch.gain = 0.0;
            ch.pan = 0.0;
            ch.fader = DEFAULT_FADER;
            ch.mute = false;
            ch.pfl = false;
            ch.to_main = true;
            ch.to_grp = [false; MAX_GROUPS];
            ch.aux_sends = [0.0; MAX_AUX_BUSES];
            ch.aux_pre = [true, true, false, false];
            ch.hpf_enabled = false;
            ch.hpf_freq = 80.0;
            ch.gate_enabled = false;
            ch.gate_threshold = -40.0;
            ch.gate_attack = 5.0;
            ch.gate_release = 100.0;
            ch.gate_range = -80.0;
            ch.comp_enabled = false;
            ch.comp_threshold = -20.0;
            ch.comp_ratio = 4.0;
            ch.comp_attack = 10.0;
            ch.comp_release = 100.0;
            ch.comp_makeup = 0.0;
            ch.comp_knee = -6.0;
            ch.eq_enabled = false;
            ch.eq_bands = [
                (80.0, 0.0, 1.0),
                (400.0, 0.0, 1.0),
                (2000.0, 0.0, 1.0),
                (8000.0, 0.0, 1.0),
            ];
        }

        self.selection = None;
        self.is_reset = true;
    }

    /// Collect only structural properties (after reset).
    /// Backend uses its own matching defaults for everything else.
    pub fn collect_structural_properties(&self) -> HashMap<String, PropertyValue> {
        let mut props = HashMap::new();
        props.insert(
            "num_channels".to_string(),
            PropertyValue::Int(self.num_channels as i64),
        );
        props.insert(
            "num_aux_buses".to_string(),
            PropertyValue::Int(self.num_aux_buses as i64),
        );
        props.insert(
            "num_groups".to_string(),
            PropertyValue::Int(self.num_groups as i64),
        );
        props
    }

    /// Show the mixer in fullscreen mode.
    pub fn show_fullscreen(&mut self, ui: &mut Ui, ctx: &Context, meter_store: &MeterDataStore) {
        // Check for save result from async task
        if let Some(status) = crate::app::get_local_storage("mixer_save_status") {
            crate::app::remove_local_storage("mixer_save_status");
            if status == "ok" {
                self.status = "Mixer state saved".to_string();
            } else {
                self.error = Some(format!("Save failed: {}", status));
            }
        }

        self.handle_keyboard(ui, ctx);

        let available_height = ui.available_height();
        let detail_panel_height = if self.selection.is_some() { 180.0 } else { 0.0 };
        let status_bar_height = 30.0;
        let channel_area_height = (available_height
            - BUS_ROW_MIN_HEIGHT
            - detail_panel_height
            - status_bar_height
            - 16.0)
            .max(300.0);

        // Outer vertical scroll wraps the entire mixer
        egui::ScrollArea::vertical()
            .id_salt("mixer_v_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    // ── Row 1: Channel strips ──
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), channel_area_height),
                        egui::Layout::left_to_right(egui::Align::Min),
                        |ui| {
                            egui::ScrollArea::horizontal()
                                .id_salt("ch_h_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        for i in 0..self.channels.len() {
                                            let meter_key =
                                                format!("{}:meter:{}", self.block_id, i + 1);
                                            let meter_data =
                                                meter_store.get(&self.flow_id, &meter_key);
                                            self.render_channel_strip(ui, ctx, i, meter_data);
                                            ui.add_space(STRIP_GAP);
                                        }
                                    });
                                });
                        },
                    );

                    ui.separator();

                    // ── Row 2: Bus masters (aux + groups + main) ──
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), BUS_ROW_MIN_HEIGHT),
                        egui::Layout::left_to_right(egui::Align::Min),
                        |ui| {
                            egui::ScrollArea::horizontal()
                                .id_salt("bus_h_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        if self.num_aux_buses > 0 {
                                            self.render_aux_masters(ui, ctx, meter_store);
                                            ui.add_space(8.0);
                                            ui.separator();
                                            ui.add_space(8.0);
                                        }

                                        if self.num_groups > 0 {
                                            self.render_group_strips(ui, ctx, meter_store);
                                            ui.add_space(8.0);
                                            ui.separator();
                                            ui.add_space(8.0);
                                        }

                                        self.render_main_strip(ui, ctx, meter_store);
                                    });
                                });
                        },
                    );

                    // ── Row 3: Detail panel (Gate/Comp/EQ) ──
                    if self.selection.is_some() {
                        ui.separator();
                        egui::ScrollArea::horizontal()
                            .id_salt("detail_h_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                self.render_detail_panel(ui, ctx);
                            });
                    }

                    // ── Status bar ──
                    ui.separator();
                    ui.horizontal(|ui| {
                        if let Some(error) = &self.error {
                            ui.colored_label(Color32::RED, error);
                        } else if !self.status.is_empty() {
                            ui.label(&self.status);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            match &self.selection {
                                Some(Selection::Channel(ch)) => {
                                    ui.label(format!("Selected: Ch {}", ch + 1));
                                }
                                Some(Selection::Main) => {
                                    ui.label("Selected: MAIN");
                                }
                                None => {}
                            }
                            ui.checkbox(&mut self.live_updates, "Live");
                            if ui.button("Save").clicked() {
                                self.save_requested = true;
                                self.status = "Saving mixer state...".to_string();
                            }
                            if ui.button("Reset").clicked() {
                                self.reset_to_defaults();
                                self.save_requested = true;
                                self.status = "Reset to defaults, saving...".to_string();
                            }
                        });
                    });
                });
            });
    }

    /// Render a single channel strip.
    fn render_channel_strip(
        &mut self,
        ui: &mut Ui,
        ctx: &Context,
        index: usize,
        meter_data: Option<&MeterData>,
    ) {
        let channel_label = self.channels[index].label.clone();
        let channel_pan = self.channels[index].pan;
        let channel_fader = self.channels[index].fader;
        let channel_mute = self.channels[index].mute;
        let channel_pfl = self.channels[index].pfl;
        let channel_gate = self.channels[index].gate_enabled;
        let channel_comp = self.channels[index].comp_enabled;
        let channel_eq = self.channels[index].eq_enabled;
        let is_selected = self.selection == Some(Selection::Channel(index));

        let frame_color = if is_selected {
            Color32::from_rgb(50, 65, 80)
        } else {
            Color32::from_rgb(38, 38, 42)
        };

        let strip_inner = self.strip_inner();

        let frame_response = egui::Frame::default()
            .fill(frame_color)
            .corner_radius(CornerRadius::same(3))
            .inner_margin(STRIP_MARGIN)
            .show(ui, |ui| {
                ui.set_min_width(strip_inner);
                ui.set_max_width(strip_inner);

                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;

                    // ── Label ──
                    ui.label(egui::RichText::new(&channel_label).strong().size(11.0));

                    // ── H / G / C / E buttons ──
                    let hgce_btn_w = (strip_inner - 10.0) / 4.0;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;

                        // HPF button
                        let channel_hpf = self.channels[index].hpf_enabled;
                        let hpf_fill = if channel_hpf {
                            Color32::from_rgb(150, 80, 150)
                        } else {
                            Color32::from_rgb(48, 48, 52)
                        };
                        let hpf_text = if channel_hpf {
                            Color32::WHITE
                        } else {
                            Color32::from_gray(120)
                        };
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new("H").small().color(hpf_text))
                                    .fill(hpf_fill)
                                    .min_size(Vec2::new(hgce_btn_w, SMALL_BTN_H)),
                            )
                            .clicked()
                        {
                            self.channels[index].hpf_enabled = !self.channels[index].hpf_enabled;
                            self.update_processing_param(ctx, index, "hpf", "enabled");
                        }

                        // G / C / E buttons
                        for (label, enabled, active_color, prop) in [
                            (
                                "G",
                                channel_gate,
                                Color32::from_rgb(0, 150, 0),
                                "gate_enabled",
                            ),
                            (
                                "C",
                                channel_comp,
                                Color32::from_rgb(180, 100, 0),
                                "comp_enabled",
                            ),
                            (
                                "E",
                                channel_eq,
                                Color32::from_rgb(0, 100, 180),
                                "eq_enabled",
                            ),
                        ] {
                            let fill = if enabled {
                                active_color
                            } else {
                                Color32::from_rgb(48, 48, 52)
                            };
                            let text_col = if enabled {
                                Color32::WHITE
                            } else {
                                Color32::from_gray(120)
                            };
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(label).small().color(text_col),
                                    )
                                    .fill(fill)
                                    .min_size(Vec2::new(hgce_btn_w, SMALL_BTN_H)),
                                )
                                .clicked()
                            {
                                match prop {
                                    "gate_enabled" => {
                                        self.channels[index].gate_enabled =
                                            !self.channels[index].gate_enabled
                                    }
                                    "comp_enabled" => {
                                        self.channels[index].comp_enabled =
                                            !self.channels[index].comp_enabled
                                    }
                                    "eq_enabled" => {
                                        self.channels[index].eq_enabled =
                                            !self.channels[index].eq_enabled
                                    }
                                    _ => {}
                                }
                                self.update_channel_property(ctx, index, prop);
                            }
                        }
                    });

                    // ── Aux send knobs ──
                    if self.num_aux_buses > 0 {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 1.0;
                            for aux_idx in 0..self.num_aux_buses.min(MAX_AUX_BUSES) {
                                let response = self.render_knob(ui, index, aux_idx);
                                if response.dragged() {
                                    self.active_control = ActiveControl::AuxSend(index, aux_idx);
                                } else if response.drag_stopped() {
                                    self.active_control = ActiveControl::None;
                                }
                            }
                        });
                    }

                    // ── Routing buttons (M + group numbers) ──
                    {
                        let num_dest = 1 + self.num_groups;
                        let btn_w =
                            (strip_inner - 4.0 - (num_dest as f32 - 1.0) * 2.0) / num_dest as f32;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            // Main
                            let to_main = self.channels[index].to_main;
                            let fill = if to_main {
                                Color32::from_rgb(70, 110, 70)
                            } else {
                                Color32::from_rgb(48, 48, 52)
                            };
                            let text_col = if to_main {
                                Color32::WHITE
                            } else {
                                Color32::from_gray(100)
                            };
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("M").small().color(text_col),
                                    )
                                    .fill(fill)
                                    .min_size(Vec2::new(btn_w, SMALL_BTN_H)),
                                )
                                .clicked()
                            {
                                self.channels[index].to_main = !to_main;
                                self.update_routing(ctx, index);
                            }
                            // Groups
                            for g in 0..self.num_groups.min(MAX_GROUPS) {
                                let on = self.channels[index].to_grp[g];
                                let fill = if on {
                                    Color32::from_rgb(140, 90, 140)
                                } else {
                                    Color32::from_rgb(48, 48, 52)
                                };
                                let text_col = if on {
                                    Color32::WHITE
                                } else {
                                    Color32::from_gray(100)
                                };
                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(format!("{}", g + 1))
                                                .small()
                                                .color(text_col),
                                        )
                                        .fill(fill)
                                        .min_size(Vec2::new(btn_w, SMALL_BTN_H)),
                                    )
                                    .clicked()
                                {
                                    self.channels[index].to_grp[g] = !on;
                                    self.update_routing(ctx, index);
                                }
                            }
                        });
                    }

                    // ── LCD display ──
                    let display_text = match &self.active_control {
                        ActiveControl::Fader(ch) if *ch == index => format_db(channel_fader),
                        ActiveControl::AuxSend(ch, aux) if *ch == index => {
                            let lvl = self.channels[index].aux_sends[*aux];
                            format!("A{} {}", aux + 1, format_db(lvl))
                        }
                        ActiveControl::Pan(ch) if *ch == index => format_pan(channel_pan),
                        _ => {
                            let db_str = format_db(channel_fader);
                            let pan_str = format_pan(channel_pan);
                            format!("{} {}", pan_str, db_str)
                        }
                    };
                    self.render_lcd(ui, &display_text, strip_inner - 4.0, LCD_H);

                    // ── Pan knob ──
                    let pan_response = self.render_pan_knob(ui, index);
                    if pan_response.dragged() {
                        self.active_control = ActiveControl::Pan(index);
                        self.update_channel_property(ctx, index, "pan");
                    } else if pan_response.drag_stopped() {
                        self.active_control = ActiveControl::None;
                    }

                    ui.add_space(2.0);

                    // ── Fader + meter + scale ──
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), FADER_HEIGHT),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            self.render_stereo_meter(ui, meter_data, FADER_HEIGHT);
                            ui.add_space(1.0);
                            self.render_db_scale(ui, FADER_HEIGHT);
                            ui.add_space(1.0);
                            let fader_response = self.render_fader(ui, index, FADER_HEIGHT);
                            if fader_response.double_clicked() {
                                self.update_channel_property(ctx, index, "fader");
                            } else if fader_response.dragged() {
                                self.active_control = ActiveControl::Fader(index);
                                self.update_channel_property(ctx, index, "fader");
                            } else if fader_response.drag_stopped() {
                                self.active_control = ActiveControl::None;
                            }
                        },
                    );

                    ui.add_space(2.0);

                    // ── Mute button ──
                    let mute_color = if channel_mute {
                        Color32::from_rgb(200, 50, 50)
                    } else {
                        Color32::from_rgb(55, 55, 60)
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("MUTE").small().color(Color32::WHITE),
                            )
                            .fill(mute_color)
                            .min_size(Vec2::new(strip_inner - 4.0, BTN_H)),
                        )
                        .clicked()
                    {
                        self.channels[index].mute = !self.channels[index].mute;
                        self.update_channel_property(ctx, index, "mute");
                    }

                    // ── PFL button ──
                    let pfl_color = if channel_pfl {
                        Color32::from_rgb(200, 200, 0)
                    } else {
                        Color32::from_rgb(48, 48, 52)
                    };
                    let pfl_text_col = if channel_pfl {
                        Color32::BLACK
                    } else {
                        Color32::from_gray(100)
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("PFL").small().color(pfl_text_col),
                            )
                            .fill(pfl_color)
                            .min_size(Vec2::new(strip_inner - 4.0, BTN_H)),
                        )
                        .clicked()
                    {
                        self.channels[index].pfl = !self.channels[index].pfl;
                        self.update_channel_property(ctx, index, "pfl");
                    }
                });
            });

        // Detect click on strip background for selection.
        // Use pointer query instead of ui.interact() to avoid stealing
        // double-click events from child widgets (like faders).
        let strip_rect = frame_response.response.rect;
        if ui.input(|i| i.pointer.any_pressed())
            && ui
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|pos| strip_rect.contains(pos))
        {
            self.selection = Some(Selection::Channel(index));
        }
    }

    /// Render the main/master strip (compact, for bus row).
    fn render_main_strip(&mut self, ui: &mut Ui, ctx: &Context, meter_store: &MeterDataStore) {
        let main_meter_key = format!("{}:meter:main", self.block_id);
        let main_meter_data = meter_store.get(&self.flow_id, &main_meter_key);
        let is_selected = self.selection == Some(Selection::Main);

        let frame_color = if is_selected {
            Color32::from_rgb(55, 55, 75)
        } else {
            Color32::from_rgb(45, 45, 55)
        };

        let mut should_select = false;

        let frame_response = egui::Frame::default()
            .fill(frame_color)
            .corner_radius(CornerRadius::same(3))
            .inner_margin(STRIP_MARGIN)
            .show(ui, |ui| {
                ui.set_min_width(BUS_STRIP_INNER);
                ui.set_max_width(BUS_STRIP_INNER);

                let bg_rect = ui.available_rect_before_wrap();
                let bg_response =
                    ui.interact(bg_rect, ui.id().with("main_strip_bg"), Sense::click());
                if bg_response.clicked() {
                    should_select = true;
                }

                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;

                    ui.label(
                        egui::RichText::new("MAIN")
                            .strong()
                            .size(12.0)
                            .color(Color32::from_rgb(200, 200, 255)),
                    );

                    self.render_lcd(
                        ui,
                        &format_db(self.main_fader),
                        BUS_STRIP_INNER - 4.0,
                        LCD_H,
                    );

                    ui.add_space(2.0);

                    // Fader + meter + scale
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), BUS_FADER_HEIGHT),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            self.render_stereo_meter(ui, main_meter_data, BUS_FADER_HEIGHT);
                            ui.add_space(1.0);
                            self.render_db_scale(ui, BUS_FADER_HEIGHT);
                            ui.add_space(1.0);

                            let mut main_fader_db = linear_to_db(self.main_fader as f64) as f32;
                            let (rect, response) = ui.allocate_exact_size(
                                Vec2::new(20.0, BUS_FADER_HEIGHT),
                                Sense::click_and_drag(),
                            );
                            if response.double_clicked() {
                                if (main_fader_db - 0.0).abs() < 0.5 {
                                    self.main_fader = 0.0;
                                    main_fader_db = -60.0;
                                } else {
                                    self.main_fader = 1.0;
                                    main_fader_db = 0.0;
                                }
                                self.update_main_fader(ctx);
                            } else if response.dragged() {
                                self.active_control = ActiveControl::MainFader;
                                let delta = -response.drag_delta().y;
                                let db_per_pixel = 66.0 / (BUS_FADER_HEIGHT - 10.0);
                                main_fader_db =
                                    (main_fader_db + delta * db_per_pixel).clamp(-60.0, 6.0);
                                self.main_fader = db_to_linear_f32(main_fader_db);
                            } else if response.drag_stopped() {
                                self.active_control = ActiveControl::None;
                            }
                            let painter = ui.painter();
                            let track_rect = Rect::from_center_size(
                                rect.center(),
                                Vec2::new(4.0, BUS_FADER_HEIGHT - 10.0),
                            );
                            painter.rect_filled(
                                track_rect,
                                CornerRadius::same(2),
                                Color32::from_gray(55),
                            );
                            let handle_y = db_to_y(main_fader_db, rect.min.y, rect.max.y);
                            let handle_rect = Rect::from_center_size(
                                egui::pos2(rect.center().x, handle_y),
                                Vec2::new(14.0, 30.0),
                            );
                            let handle_color = if response.dragged() {
                                Color32::from_rgb(100, 140, 240)
                            } else if response.hovered() {
                                Color32::from_rgb(190, 190, 200)
                            } else {
                                Color32::from_rgb(155, 155, 165)
                            };
                            painter.rect_filled(handle_rect, CornerRadius::same(3), handle_color);
                            painter.line_segment(
                                [
                                    egui::pos2(handle_rect.left() + 2.0, handle_y),
                                    egui::pos2(handle_rect.right() - 2.0, handle_y),
                                ],
                                Stroke::new(1.5, Color32::from_gray(40)),
                            );
                            if response.drag_stopped() || response.dragged() {
                                self.update_main_fader(ctx);
                            }
                        },
                    );

                    ui.add_space(2.0);

                    let mute_color = if self.main_mute {
                        Color32::from_rgb(200, 50, 50)
                    } else {
                        Color32::from_rgb(55, 55, 60)
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("MUTE").small().color(Color32::WHITE),
                            )
                            .fill(mute_color)
                            .min_size(Vec2::new(BUS_STRIP_INNER - 4.0, BTN_H)),
                        )
                        .clicked()
                    {
                        self.main_mute = !self.main_mute;
                        self.update_main_mute(ctx);
                    }
                });
            });

        if should_select {
            self.selection = Some(Selection::Main);
        }
        let _ = frame_response;
    }

    /// Render the group strips section (compact, for bus row).
    fn render_group_strips(&mut self, ui: &mut Ui, ctx: &Context, meter_store: &MeterDataStore) {
        for sg_idx in 0..self.num_groups.min(MAX_GROUPS) {
            while self.groups.len() <= sg_idx {
                self.groups.push(GroupStrip::new(self.groups.len()));
            }

            let meter_key = format!("{}:meter:group{}", self.block_id, sg_idx + 1);
            let meter_data = meter_store.get(&self.flow_id, &meter_key);

            egui::Frame::default()
                .fill(Color32::from_rgb(42, 38, 48))
                .corner_radius(CornerRadius::same(3))
                .inner_margin(STRIP_MARGIN)
                .show(ui, |ui| {
                    ui.set_min_width(BUS_STRIP_INNER);
                    ui.set_max_width(BUS_STRIP_INNER);

                    ui.vertical_centered(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;

                        ui.label(
                            egui::RichText::new(format!("GRP{}", sg_idx + 1))
                                .strong()
                                .size(11.0)
                                .color(Color32::from_rgb(200, 150, 200)),
                        );

                        self.render_lcd(
                            ui,
                            &format_db(self.groups[sg_idx].fader),
                            BUS_STRIP_INNER - 4.0,
                            LCD_H,
                        );

                        ui.add_space(2.0);

                        ui.allocate_ui_with_layout(
                            Vec2::new(ui.available_width(), BUS_FADER_HEIGHT),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                self.render_stereo_meter(ui, meter_data, BUS_FADER_HEIGHT);
                                ui.add_space(1.0);
                                self.render_db_scale(ui, BUS_FADER_HEIGHT);
                                ui.add_space(1.0);
                                let fader_response =
                                    self.render_group_fader(ui, sg_idx, BUS_FADER_HEIGHT);
                                if fader_response.double_clicked() {
                                    self.update_group_fader(ctx, sg_idx);
                                } else if fader_response.dragged() {
                                    self.active_control = ActiveControl::GroupFader(sg_idx);
                                    self.update_group_fader(ctx, sg_idx);
                                } else if fader_response.drag_stopped() {
                                    self.active_control = ActiveControl::None;
                                }
                            },
                        );

                        ui.add_space(2.0);

                        let mute = self.groups[sg_idx].mute;
                        let mute_color = if mute {
                            Color32::from_rgb(200, 50, 50)
                        } else {
                            Color32::from_rgb(55, 55, 60)
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("MUTE").small().color(Color32::WHITE),
                                )
                                .fill(mute_color)
                                .min_size(Vec2::new(BUS_STRIP_INNER - 4.0, BTN_H)),
                            )
                            .clicked()
                        {
                            self.groups[sg_idx].mute = !self.groups[sg_idx].mute;
                            self.update_group_mute(ctx, sg_idx);
                        }
                    });
                });

            ui.add_space(STRIP_GAP);
        }
    }

    /// Render the aux master section (compact, for bus row).
    fn render_aux_masters(&mut self, ui: &mut Ui, ctx: &Context, meter_store: &MeterDataStore) {
        for aux_idx in 0..self.num_aux_buses.min(MAX_AUX_BUSES) {
            while self.aux_masters.len() <= aux_idx {
                self.aux_masters
                    .push(AuxMaster::new(self.aux_masters.len()));
            }

            let meter_key = format!("{}:meter:aux{}", self.block_id, aux_idx + 1);
            let meter_data = meter_store.get(&self.flow_id, &meter_key);

            egui::Frame::default()
                .fill(Color32::from_rgb(38, 42, 50))
                .corner_radius(CornerRadius::same(3))
                .inner_margin(STRIP_MARGIN)
                .show(ui, |ui| {
                    ui.set_min_width(BUS_STRIP_INNER);
                    ui.set_max_width(BUS_STRIP_INNER);

                    ui.vertical_centered(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;

                        ui.label(
                            egui::RichText::new(format!("AUX{}", aux_idx + 1))
                                .strong()
                                .size(11.0)
                                .color(Color32::from_rgb(150, 200, 255)),
                        );

                        self.render_lcd(
                            ui,
                            &format_db(self.aux_masters[aux_idx].fader),
                            BUS_STRIP_INNER - 4.0,
                            LCD_H,
                        );

                        ui.add_space(2.0);

                        ui.allocate_ui_with_layout(
                            Vec2::new(ui.available_width(), BUS_FADER_HEIGHT),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                self.render_stereo_meter(ui, meter_data, BUS_FADER_HEIGHT);
                                ui.add_space(1.0);
                                self.render_db_scale(ui, BUS_FADER_HEIGHT);
                                ui.add_space(1.0);
                                let fader_response =
                                    self.render_aux_master_fader(ui, aux_idx, BUS_FADER_HEIGHT);
                                if fader_response.double_clicked() {
                                    self.update_aux_master_fader(ctx, aux_idx);
                                } else if fader_response.dragged() {
                                    self.active_control = ActiveControl::AuxMasterFader(aux_idx);
                                    self.update_aux_master_fader(ctx, aux_idx);
                                } else if fader_response.drag_stopped() {
                                    self.active_control = ActiveControl::None;
                                }
                            },
                        );

                        ui.add_space(2.0);

                        let mute = self.aux_masters[aux_idx].mute;
                        let mute_color = if mute {
                            Color32::from_rgb(200, 50, 50)
                        } else {
                            Color32::from_rgb(55, 55, 60)
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("MUTE").small().color(Color32::WHITE),
                                )
                                .fill(mute_color)
                                .min_size(Vec2::new(BUS_STRIP_INNER - 4.0, BTN_H)),
                            )
                            .clicked()
                        {
                            self.aux_masters[aux_idx].mute = !self.aux_masters[aux_idx].mute;
                            self.update_aux_master_mute(ctx, aux_idx);
                        }
                    });
                });

            ui.add_space(STRIP_GAP);
        }
    }

    /// Render a group fader.
    fn render_group_fader(&mut self, ui: &mut Ui, sg_idx: usize, height: f32) -> Response {
        let fader_val = self.groups[sg_idx].fader;
        let mut fader_db = linear_to_db(fader_val as f64) as f32;

        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(16.0, height), Sense::click_and_drag());

        if response.double_clicked() {
            if (fader_db - 0.0).abs() < 0.5 {
                self.groups[sg_idx].fader = 0.0;
                fader_db = -60.0;
            } else {
                self.groups[sg_idx].fader = 1.0;
                fader_db = 0.0;
            }
        } else if response.dragged() {
            let delta = -response.drag_delta().y;
            let db_per_pixel = 66.0 / (height - 10.0);
            fader_db = (fader_db + delta * db_per_pixel).clamp(-60.0, 6.0);
            self.groups[sg_idx].fader = db_to_linear_f32(fader_db);
        }

        // Draw fader track
        let painter = ui.painter();
        let track_rect = Rect::from_center_size(rect.center(), Vec2::new(4.0, height - 10.0));
        painter.rect_filled(track_rect, CornerRadius::same(2), Color32::from_gray(60));

        // Draw handle
        let handle_y = db_to_y(fader_db, rect.min.y, rect.max.y);
        let handle_rect =
            Rect::from_center_size(egui::pos2(rect.center().x, handle_y), Vec2::new(12.0, 30.0));
        let handle_color = if response.dragged() {
            Color32::from_rgb(200, 150, 200)
        } else if response.hovered() {
            Color32::from_rgb(200, 200, 200)
        } else {
            Color32::from_rgb(160, 160, 160)
        };
        painter.rect_filled(handle_rect, CornerRadius::same(3), handle_color);
        painter.line_segment(
            [
                egui::pos2(handle_rect.left() + 2.0, handle_y),
                egui::pos2(handle_rect.right() - 2.0, handle_y),
            ],
            Stroke::new(1.5, Color32::from_gray(40)),
        );

        response
    }

    /// Render an aux master fader.
    fn render_aux_master_fader(&mut self, ui: &mut Ui, aux_idx: usize, height: f32) -> Response {
        let fader_val = self.aux_masters[aux_idx].fader;
        let mut fader_db = linear_to_db(fader_val as f64) as f32;

        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(16.0, height), Sense::click_and_drag());

        if response.double_clicked() {
            if (fader_db - 0.0).abs() < 0.5 {
                self.aux_masters[aux_idx].fader = 0.0;
                fader_db = -60.0;
            } else {
                self.aux_masters[aux_idx].fader = 1.0;
                fader_db = 0.0;
            }
        } else if response.dragged() {
            let delta = -response.drag_delta().y;
            let db_per_pixel = 66.0 / (height - 10.0);
            fader_db = (fader_db + delta * db_per_pixel).clamp(-60.0, 6.0);
            self.aux_masters[aux_idx].fader = db_to_linear_f32(fader_db);
        }

        // Draw fader track
        let painter = ui.painter();
        let track_rect = Rect::from_center_size(rect.center(), Vec2::new(4.0, height - 10.0));
        painter.rect_filled(track_rect, CornerRadius::same(2), Color32::from_gray(60));

        // Draw handle
        let handle_y = db_to_y(fader_db, rect.min.y, rect.max.y);
        let handle_rect =
            Rect::from_center_size(egui::pos2(rect.center().x, handle_y), Vec2::new(12.0, 30.0));
        let handle_color = if response.dragged() {
            Color32::from_rgb(150, 200, 255)
        } else if response.hovered() {
            Color32::from_rgb(200, 200, 200)
        } else {
            Color32::from_rgb(160, 160, 160)
        };
        painter.rect_filled(handle_rect, CornerRadius::same(3), handle_color);
        painter.line_segment(
            [
                egui::pos2(handle_rect.left() + 2.0, handle_y),
                egui::pos2(handle_rect.right() - 2.0, handle_y),
            ],
            Stroke::new(1.5, Color32::from_gray(40)),
        );

        response
    }

    /// Render a pan knob. Center (0.0) at 12 o'clock, L at 7:30, R at 4:30.
    fn render_pan_knob(&mut self, ui: &mut Ui, index: usize) -> Response {
        let pan = self.channels[index].pan;
        let (rect, response) =
            ui.allocate_exact_size(Vec2::splat(PAN_KNOB_SIZE), Sense::click_and_drag());

        if response.dragged() {
            let delta = -response.drag_delta().y * 0.01;
            self.channels[index].pan = (self.channels[index].pan + delta).clamp(-1.0, 1.0);
        }

        // Double-click: reset to center
        if response.double_clicked() {
            self.channels[index].pan = 0.0;
        }

        let painter = ui.painter();
        let center = rect.center();
        let radius = PAN_KNOB_SIZE * 0.5 - 1.0;

        // Background circle
        painter.circle_filled(center, radius, Color32::from_rgb(28, 28, 32));

        // Pan maps: -1.0 → 0.0 (7:30), 0.0 → 0.5 (12 o'clock), 1.0 → 1.0 (4:30)
        let normalized = (pan + 1.0) * 0.5;

        let arc_start = std::f32::consts::PI * 1.75; // 7:30
        let arc_sweep = std::f32::consts::PI * 1.5; // 270°

        // Draw arc from center (12 o'clock) to current pan position
        let center_norm = 0.5;
        let (from_norm, to_norm) = if normalized < center_norm {
            (normalized, center_norm)
        } else {
            (center_norm, normalized)
        };

        if (to_norm - from_norm) > 0.005 {
            let from_angle = arc_start + from_norm * arc_sweep;
            let to_angle = arc_start + to_norm * arc_sweep;
            let sweep = to_angle - from_angle;
            let segments = (sweep.abs() / arc_sweep * 24.0).max(4.0) as usize;
            let arc_color = if response.dragged() {
                Color32::from_rgb(255, 180, 100)
            } else {
                Color32::from_rgb(200, 140, 70)
            };
            let arc_r = radius - 1.5;
            for i in 0..segments {
                let t0 = i as f32 / segments as f32;
                let t1 = (i + 1) as f32 / segments as f32;
                let a0 = from_angle + sweep * t0;
                let a1 = from_angle + sweep * t1;
                painter.line_segment(
                    [
                        egui::pos2(center.x - a0.cos() * arc_r, center.y - a0.sin() * arc_r),
                        egui::pos2(center.x - a1.cos() * arc_r, center.y - a1.sin() * arc_r),
                    ],
                    Stroke::new(2.5, arc_color),
                );
            }
        }

        // Center marker tick at 12 o'clock
        let center_angle = arc_start + 0.5 * arc_sweep;
        let tick_inner = radius - 3.5;
        let tick_outer = radius + 0.5;
        painter.line_segment(
            [
                egui::pos2(
                    center.x - center_angle.cos() * tick_inner,
                    center.y - center_angle.sin() * tick_inner,
                ),
                egui::pos2(
                    center.x - center_angle.cos() * tick_outer,
                    center.y - center_angle.sin() * tick_outer,
                ),
            ],
            Stroke::new(1.0, Color32::from_gray(90)),
        );

        // Pointer line
        let pointer_angle = arc_start + normalized * arc_sweep;
        let inner_r = radius * 0.25;
        let outer_r = radius - 3.0;
        let pointer_color = Color32::WHITE;
        painter.line_segment(
            [
                egui::pos2(
                    center.x - pointer_angle.cos() * inner_r,
                    center.y - pointer_angle.sin() * inner_r,
                ),
                egui::pos2(
                    center.x - pointer_angle.cos() * outer_r,
                    center.y - pointer_angle.sin() * outer_r,
                ),
            ],
            Stroke::new(1.5, pointer_color),
        );

        // Border
        let border_color = if response.hovered() || response.dragged() {
            Color32::from_rgb(200, 150, 80)
        } else {
            Color32::from_gray(55)
        };
        painter.circle_stroke(center, radius, Stroke::new(1.0, border_color));

        // Tooltip
        let resp = response.on_hover_text(format!("Pan: {}", format_pan(pan)));
        resp
    }

    /// Render a styled LCD display box.
    fn render_lcd(&self, ui: &mut Ui, text: &str, width: f32, height: f32) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(2), Color32::from_rgb(18, 22, 28));
        painter.rect_stroke(
            rect,
            CornerRadius::same(2),
            Stroke::new(1.0, Color32::from_rgb(45, 50, 55)),
            egui::epaint::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::monospace(10.0),
            Color32::from_rgb(90, 200, 90),
        );
    }

    /// Render an aux send rotary knob for a channel.
    ///
    /// dB-scaled arc: -inf at 7:30 (CCW), 0 dB (unity) at 12 o'clock, +6 dB at 4:30 (CW).
    /// The first half of the 270° arc covers -60..0 dB, the second half covers 0..+6 dB.
    fn render_knob(&mut self, ui: &mut Ui, ch_idx: usize, aux_idx: usize) -> Response {
        let aux_level = self.channels[ch_idx].aux_sends[aux_idx];
        let (rect, response) =
            ui.allocate_exact_size(Vec2::splat(KNOB_SIZE), Sense::click_and_drag());

        // Drag in dB space for natural feel
        if response.dragged() {
            let current_db = linear_to_db(aux_level as f64) as f32;
            // Sensitivity: ~1 dB per 3 pixels
            let db_delta = -response.drag_delta().y * 0.35;
            let new_db = (current_db + db_delta).clamp(-60.0, 6.0);
            self.channels[ch_idx].aux_sends[aux_idx] = db_to_linear_f32(new_db);
            self.update_aux_send(ui.ctx(), ch_idx, aux_idx);
        }

        // Double-click: toggle between off and unity
        if response.double_clicked() {
            self.channels[ch_idx].aux_sends[aux_idx] = if aux_level > 0.01 { 0.0 } else { 1.0 };
            self.update_aux_send(ui.ctx(), ch_idx, aux_idx);
        }

        let painter = ui.painter();
        let center = rect.center();
        let radius = KNOB_SIZE * 0.5 - 1.0;

        // Background
        painter.circle_filled(center, radius, Color32::from_rgb(28, 28, 32));

        // Convert level to arc position (dB-scaled, 0 dB at center)
        let level = self.channels[ch_idx].aux_sends[aux_idx];
        let normalized = knob_linear_to_normalized(level);

        // Arc geometry: 270° sweep starting at 7:30 (bottom-left)
        // In our coord system (x = cx - cos*r, y = cy - sin*r):
        //   12 o'clock = π/2, clockwise = increasing angle
        //   7:30 = 7π/4, 4:30 = 5π/4 + 2π = 13π/4
        let arc_start = std::f32::consts::PI * 1.75; // 7:30 position
        let arc_sweep = std::f32::consts::PI * 1.5; // 270°

        // Draw filled arc up to current level
        if normalized > 0.005 {
            let sweep = normalized * arc_sweep;
            let segments = (normalized * 24.0).max(4.0) as usize;
            let arc_color = if response.dragged() {
                Color32::from_rgb(130, 190, 255)
            } else {
                Color32::from_rgb(90, 145, 200)
            };
            let arc_r = radius - 1.5;
            for i in 0..segments {
                let t0 = i as f32 / segments as f32;
                let t1 = (i + 1) as f32 / segments as f32;
                let a0 = arc_start + sweep * t0;
                let a1 = arc_start + sweep * t1;
                painter.line_segment(
                    [
                        egui::pos2(center.x - a0.cos() * arc_r, center.y - a0.sin() * arc_r),
                        egui::pos2(center.x - a1.cos() * arc_r, center.y - a1.sin() * arc_r),
                    ],
                    Stroke::new(2.5, arc_color),
                );
            }
        }

        // Unity marker (small tick at 12 o'clock = center of arc)
        let unity_angle = arc_start + 0.5 * arc_sweep; // 12 o'clock
        let tick_inner = radius - 3.5;
        let tick_outer = radius + 0.5;
        painter.line_segment(
            [
                egui::pos2(
                    center.x - unity_angle.cos() * tick_inner,
                    center.y - unity_angle.sin() * tick_inner,
                ),
                egui::pos2(
                    center.x - unity_angle.cos() * tick_outer,
                    center.y - unity_angle.sin() * tick_outer,
                ),
            ],
            Stroke::new(1.0, Color32::from_gray(90)),
        );

        // Pointer indicator line
        let pointer_angle = arc_start + normalized * arc_sweep;
        let inner_r = radius * 0.25;
        let outer_r = radius - 3.0;
        let pointer_color = if level > 0.01 {
            Color32::WHITE
        } else {
            Color32::from_gray(70)
        };
        painter.line_segment(
            [
                egui::pos2(
                    center.x - pointer_angle.cos() * inner_r,
                    center.y - pointer_angle.sin() * inner_r,
                ),
                egui::pos2(
                    center.x - pointer_angle.cos() * outer_r,
                    center.y - pointer_angle.sin() * outer_r,
                ),
            ],
            Stroke::new(1.5, pointer_color),
        );

        // Border
        let border_color = if response.hovered() || response.dragged() {
            Color32::from_rgb(90, 140, 200)
        } else {
            Color32::from_gray(55)
        };
        painter.circle_stroke(center, radius, Stroke::new(1.0, border_color));

        // Hover tooltip
        let db_str = if level > 0.001 {
            format!("{:.1} dB", 20.0 * level.log10())
        } else {
            "-inf".to_string()
        };
        let resp = response.on_hover_text(format!("Aux {} send: {}", aux_idx + 1, db_str));

        resp
    }

    /// Render a vertical fader using dB scale.
    /// Internally converts between dB (-60 to +6) and linear (0.0 to 2.0).
    fn render_fader(&mut self, ui: &mut Ui, index: usize, height: f32) -> Response {
        let channel = &mut self.channels[index];

        // Convert linear fader value to dB for display
        let mut fader_db = linear_to_db(channel.fader as f64) as f32;

        // Allocate exact size for the fader
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(16.0, height), Sense::click_and_drag());

        // Double-click: toggle between 0 dB and -inf
        if response.double_clicked() {
            if (fader_db - 0.0).abs() < 0.5 {
                channel.fader = 0.0;
                fader_db = -60.0;
            } else {
                channel.fader = 1.0;
                fader_db = 0.0;
            }
        } else if response.dragged() {
            // Calculate new dB value based on drag position
            let delta = -response.drag_delta().y; // Negative because y increases downward
            let db_per_pixel = 66.0 / (height - 10.0); // -60 to +6 = 66 dB range
            fader_db = (fader_db + delta * db_per_pixel).clamp(-60.0, 6.0);
            channel.fader = db_to_linear_f32(fader_db);
        }

        // Draw fader track
        let painter = ui.painter();
        let track_width = 4.0;
        let track_rect =
            Rect::from_center_size(rect.center(), Vec2::new(track_width, height - 10.0));
        painter.rect_filled(track_rect, CornerRadius::same(2), Color32::from_gray(60));

        // Draw fader handle
        let handle_y = db_to_y(fader_db, rect.min.y, rect.max.y);
        let handle_rect =
            Rect::from_center_size(egui::pos2(rect.center().x, handle_y), Vec2::new(14.0, 36.0));
        let handle_color = if response.dragged() {
            Color32::from_rgb(100, 150, 255)
        } else if response.hovered() {
            Color32::from_rgb(200, 200, 200)
        } else {
            Color32::from_rgb(160, 160, 160)
        };
        painter.rect_filled(handle_rect, CornerRadius::same(3), handle_color);
        // Center line indicating exact value
        painter.line_segment(
            [
                egui::pos2(handle_rect.left() + 2.0, handle_y),
                egui::pos2(handle_rect.right() - 2.0, handle_y),
            ],
            Stroke::new(1.5, Color32::from_gray(40)),
        );

        response
    }

    /// Render a stereo vertical meter (L/R side by side).
    fn render_stereo_meter(&self, ui: &mut Ui, meter_data: Option<&MeterData>, height: f32) {
        let bar_width = 6.0;
        let gap = 2.0;
        let total_width = bar_width * 2.0 + gap;
        let (rect, _response) =
            ui.allocate_exact_size(Vec2::new(total_width, height), Sense::hover());

        let painter = ui.painter();

        // Left channel rect
        let left_rect = Rect::from_min_size(rect.min, Vec2::new(bar_width, height));
        // Right channel rect
        let right_rect = Rect::from_min_size(
            egui::pos2(rect.min.x + bar_width + gap, rect.min.y),
            Vec2::new(bar_width, height),
        );

        // Background for both
        painter.rect_filled(
            left_rect,
            CornerRadius::same(2),
            Color32::from_rgb(20, 20, 20),
        );
        painter.rect_filled(
            right_rect,
            CornerRadius::same(2),
            Color32::from_rgb(20, 20, 20),
        );

        if let Some(data) = meter_data {
            // Get L/R peak values (or use same for both if mono)
            let (left_peak, right_peak) = if data.peak.len() >= 2 {
                (data.peak[0], data.peak[1])
            } else if !data.peak.is_empty() {
                (data.peak[0], data.peak[0])
            } else {
                return;
            };

            let bottom_y = db_to_y(-60.0, rect.min.y, rect.max.y);

            // Draw left channel
            let left_level = db_to_level(left_peak);
            let left_top_y = db_to_y(left_peak as f32, rect.min.y, rect.max.y);
            let left_bar_rect = Rect::from_min_max(
                egui::pos2(left_rect.min.x, left_top_y),
                egui::pos2(left_rect.max.x, bottom_y),
            );
            painter.rect_filled(
                left_bar_rect,
                CornerRadius::same(2),
                level_to_color(left_level),
            );

            // Draw right channel
            let right_level = db_to_level(right_peak);
            let right_top_y = db_to_y(right_peak as f32, rect.min.y, rect.max.y);
            let right_bar_rect = Rect::from_min_max(
                egui::pos2(right_rect.min.x, right_top_y),
                egui::pos2(right_rect.max.x, bottom_y),
            );
            painter.rect_filled(
                right_bar_rect,
                CornerRadius::same(2),
                level_to_color(right_level),
            );

            // Draw decay lines if available
            if data.decay.len() >= 2 {
                let left_decay_y = db_to_y(data.decay[0] as f32, rect.min.y, rect.max.y);
                painter.line_segment(
                    [
                        egui::pos2(left_rect.min.x, left_decay_y),
                        egui::pos2(left_rect.max.x, left_decay_y),
                    ],
                    Stroke::new(1.0, Color32::WHITE),
                );

                let right_decay_y = db_to_y(data.decay[1] as f32, rect.min.y, rect.max.y);
                painter.line_segment(
                    [
                        egui::pos2(right_rect.min.x, right_decay_y),
                        egui::pos2(right_rect.max.x, right_decay_y),
                    ],
                    Stroke::new(1.0, Color32::WHITE),
                );
            }
        }

        // Borders
        painter.rect_stroke(
            left_rect,
            CornerRadius::same(2),
            Stroke::new(1.0, Color32::from_gray(60)),
            egui::epaint::StrokeKind::Inside,
        );
        painter.rect_stroke(
            right_rect,
            CornerRadius::same(2),
            Stroke::new(1.0, Color32::from_gray(60)),
            egui::epaint::StrokeKind::Inside,
        );
    }

    /// Render a dB scale next to the fader.
    /// Scale is in dB with equal visual spacing for equal dB steps.
    fn render_db_scale(&self, ui: &mut Ui, height: f32) {
        let width = 18.0;
        let (rect, _response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());

        let painter = ui.painter();

        let marks: &[f32] = &[6.0, 0.0, -6.0, -12.0, -20.0, -30.0, -40.0, -60.0];
        let text_color = Color32::from_gray(140);

        for &db in marks {
            let y = db_to_y(db, rect.min.y, rect.max.y);

            let label = if db > 0.0 {
                format!("+{}", db as i32)
            } else {
                format!("{}", db as i32)
            };

            painter.line_segment(
                [egui::pos2(rect.max.x - 3.0, y), egui::pos2(rect.max.x, y)],
                Stroke::new(1.0, Color32::from_gray(100)),
            );

            painter.text(
                egui::pos2(rect.min.x, y),
                egui::Align2::LEFT_CENTER,
                &label,
                egui::FontId::proportional(8.0),
                text_color,
            );
        }
    }

    /// Render the detail panel for the current selection.
    fn render_detail_panel(&mut self, ui: &mut Ui, ctx: &Context) {
        match self.selection.clone() {
            Some(Selection::Channel(index)) => {
                self.render_channel_detail_panel(ui, ctx, index);
            }
            Some(Selection::Main) => {
                self.render_main_detail_panel(ui, ctx);
            }
            None => {}
        }
    }

    /// Render the detail panel for a selected channel (HPF/Gate/Comp/EQ).
    fn render_channel_detail_panel(&mut self, ui: &mut Ui, ctx: &Context, index: usize) {
        let ch_num = index + 1;

        egui::Frame::default()
            .fill(Color32::from_rgb(35, 35, 40))
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("Channel {} - Processing", ch_num)).strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            self.selection = None;
                        }
                    });
                });

                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    // Gain section
                    self.render_gain_section(ui, ctx, index);

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(16.0);

                    // HPF section
                    self.render_hpf_section(ui, ctx, index);

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(16.0);

                    // Gate section
                    self.render_gate_section(ui, ctx, index);

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(16.0);

                    // Compressor section
                    self.render_comp_section(ui, ctx, index);

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(16.0);

                    // EQ section
                    self.render_eq_section(ui, ctx, index);
                });
            });
    }

    /// Render the detail panel for the main bus (Comp/EQ/Limiter).
    fn render_main_detail_panel(&mut self, ui: &mut Ui, ctx: &Context) {
        egui::Frame::default()
            .fill(Color32::from_rgb(35, 35, 40))
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("MAIN - Processing")
                            .strong()
                            .color(Color32::from_rgb(200, 200, 255)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            self.selection = None;
                        }
                    });
                });

                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    // Compressor section
                    self.render_main_comp_section(ui, ctx);

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(16.0);

                    // EQ section
                    self.render_main_eq_section(ui, ctx);

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(16.0);

                    // Limiter section
                    self.render_main_limiter_section(ui, ctx);
                });
            });
    }

    /// Render main bus compressor controls.
    fn render_main_comp_section(&mut self, ui: &mut Ui, ctx: &Context) {
        ui.vertical(|ui| {
            let enabled = self.main_comp_enabled;
            let header_color = if enabled {
                Color32::from_rgb(180, 100, 0)
            } else {
                Color32::GRAY
            };

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("COMPRESSOR")
                        .color(header_color)
                        .strong(),
                );
                if ui.checkbox(&mut self.main_comp_enabled, "").changed() {
                    self.update_main_processing_param(ctx, "comp", "enabled");
                }
            });

            ui.add_space(4.0);
            if !enabled {
                ui.disable();
            }

            ui.horizontal(|ui| {
                ui.label("Thresh:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.main_comp_threshold)
                            .range(-60.0..=0.0)
                            .suffix(" dB")
                            .speed(0.5),
                    )
                    .changed()
                {
                    self.update_main_processing_param(ctx, "comp", "threshold");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Ratio:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.main_comp_ratio)
                            .range(1.0..=20.0)
                            .suffix(":1")
                            .speed(0.1),
                    )
                    .changed()
                {
                    self.update_main_processing_param(ctx, "comp", "ratio");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Attack:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.main_comp_attack)
                            .range(0.1..=200.0)
                            .suffix(" ms")
                            .speed(0.5),
                    )
                    .changed()
                {
                    self.update_main_processing_param(ctx, "comp", "attack");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Release:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.main_comp_release)
                            .range(10.0..=1000.0)
                            .suffix(" ms")
                            .speed(1.0),
                    )
                    .changed()
                {
                    self.update_main_processing_param(ctx, "comp", "release");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Makeup:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.main_comp_makeup)
                            .range(0.0..=24.0)
                            .suffix(" dB")
                            .speed(0.2),
                    )
                    .changed()
                {
                    self.update_main_processing_param(ctx, "comp", "makeup");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Knee:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.main_comp_knee)
                            .range(-24.0..=0.0)
                            .suffix(" dB")
                            .speed(0.2),
                    )
                    .changed()
                {
                    self.update_main_processing_param(ctx, "comp", "knee");
                }
            });
        });
    }

    /// Render main bus EQ controls.
    fn render_main_eq_section(&mut self, ui: &mut Ui, ctx: &Context) {
        ui.vertical(|ui| {
            let enabled = self.main_eq_enabled;
            let header_color = if enabled {
                Color32::from_rgb(0, 100, 180)
            } else {
                Color32::GRAY
            };

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("EQ").color(header_color).strong());
                if ui.checkbox(&mut self.main_eq_enabled, "").changed() {
                    self.update_main_processing_param(ctx, "eq", "enabled");
                }
            });

            ui.add_space(4.0);
            if !enabled {
                ui.disable();
            }

            let band_names = ["Low", "Lo-Mid", "Hi-Mid", "High"];

            ui.horizontal(|ui| {
                for (band, name) in band_names.iter().enumerate() {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(*name).small());

                        if ui
                            .add(
                                egui::DragValue::new(&mut self.main_eq_bands[band].0)
                                    .range(20.0..=20000.0)
                                    .suffix(" Hz")
                                    .speed(10.0),
                            )
                            .changed()
                        {
                            self.update_main_eq_param(ctx, band, "freq");
                        }

                        if ui
                            .add(
                                egui::DragValue::new(&mut self.main_eq_bands[band].1)
                                    .range(-15.0..=15.0)
                                    .suffix(" dB")
                                    .speed(0.1),
                            )
                            .changed()
                        {
                            self.update_main_eq_param(ctx, band, "gain");
                        }

                        if ui
                            .add(
                                egui::DragValue::new(&mut self.main_eq_bands[band].2)
                                    .range(0.1..=10.0)
                                    .prefix("Q ")
                                    .speed(0.05),
                            )
                            .changed()
                        {
                            self.update_main_eq_param(ctx, band, "q");
                        }
                    });

                    if band < 3 {
                        ui.add_space(8.0);
                    }
                }
            });
        });
    }

    /// Render main bus limiter controls.
    fn render_main_limiter_section(&mut self, ui: &mut Ui, ctx: &Context) {
        ui.vertical(|ui| {
            let enabled = self.main_limiter_enabled;
            let header_color = if enabled {
                Color32::from_rgb(200, 60, 60)
            } else {
                Color32::GRAY
            };

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("LIMITER").color(header_color).strong());
                if ui.checkbox(&mut self.main_limiter_enabled, "").changed() {
                    self.update_main_processing_param(ctx, "limiter", "enabled");
                }
            });

            ui.add_space(4.0);
            if !enabled {
                ui.disable();
            }

            ui.horizontal(|ui| {
                ui.label("Thresh:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.main_limiter_threshold)
                            .range(-20.0..=0.0)
                            .suffix(" dB")
                            .speed(0.2),
                    )
                    .changed()
                {
                    self.update_main_processing_param(ctx, "limiter", "threshold");
                }
            });
        });
    }

    /// Render Gate controls.
    /// Render HPF controls.
    /// Render input gain control.
    fn render_gain_section(&mut self, ui: &mut Ui, ctx: &Context, index: usize) {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("GAIN")
                    .color(Color32::from_rgb(200, 200, 200))
                    .strong(),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Gain:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].gain)
                            .range(-20.0..=20.0)
                            .suffix(" dB")
                            .speed(0.2),
                    )
                    .changed()
                {
                    self.update_channel_property(ctx, index, "gain");
                }
            });
        });
    }

    /// Render HPF controls.
    fn render_hpf_section(&mut self, ui: &mut Ui, ctx: &Context, index: usize) {
        ui.vertical(|ui| {
            let enabled = self.channels[index].hpf_enabled;
            let header_color = if enabled {
                Color32::from_rgb(150, 80, 150)
            } else {
                Color32::GRAY
            };

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("HPF").color(header_color).strong());
                if ui
                    .checkbox(&mut self.channels[index].hpf_enabled, "")
                    .changed()
                {
                    self.update_processing_param(ctx, index, "hpf", "enabled");
                }
            });

            ui.add_space(4.0);
            if !enabled {
                ui.disable();
            }

            ui.horizontal(|ui| {
                ui.label("Cutoff:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].hpf_freq)
                            .range(20.0..=500.0)
                            .suffix(" Hz")
                            .speed(1.0),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "hpf", "freq");
                }
            });
        });
    }

    /// Render Gate controls.
    fn render_gate_section(&mut self, ui: &mut Ui, ctx: &Context, index: usize) {
        ui.vertical(|ui| {
            let enabled = self.channels[index].gate_enabled;
            let header_color = if enabled {
                Color32::from_rgb(0, 150, 0)
            } else {
                Color32::GRAY
            };

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("GATE").color(header_color).strong());
                if ui
                    .checkbox(&mut self.channels[index].gate_enabled, "")
                    .changed()
                {
                    self.update_channel_property(ctx, index, "gate_enabled");
                }
            });

            ui.add_space(4.0);
            if !enabled {
                ui.disable();
            }

            ui.horizontal(|ui| {
                ui.label("Thresh:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].gate_threshold)
                            .range(-60.0..=0.0)
                            .suffix(" dB")
                            .speed(0.5),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "gate", "threshold");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Attack:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].gate_attack)
                            .range(0.1..=200.0)
                            .suffix(" ms")
                            .speed(0.5),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "gate", "attack");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Release:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].gate_release)
                            .range(10.0..=1000.0)
                            .suffix(" ms")
                            .speed(1.0),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "gate", "release");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Range:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].gate_range)
                            .range(-80.0..=0.0)
                            .suffix(" dB")
                            .speed(0.5),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "gate", "range");
                }
            });
        });
    }

    /// Render Compressor controls.
    fn render_comp_section(&mut self, ui: &mut Ui, ctx: &Context, index: usize) {
        ui.vertical(|ui| {
            let enabled = self.channels[index].comp_enabled;
            let header_color = if enabled {
                Color32::from_rgb(180, 100, 0)
            } else {
                Color32::GRAY
            };

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("COMPRESSOR")
                        .color(header_color)
                        .strong(),
                );
                if ui
                    .checkbox(&mut self.channels[index].comp_enabled, "")
                    .changed()
                {
                    self.update_channel_property(ctx, index, "comp_enabled");
                }
            });

            ui.add_space(4.0);
            if !enabled {
                ui.disable();
            }

            ui.horizontal(|ui| {
                ui.label("Thresh:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].comp_threshold)
                            .range(-60.0..=0.0)
                            .suffix(" dB")
                            .speed(0.5),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "comp", "threshold");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Ratio:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].comp_ratio)
                            .range(1.0..=20.0)
                            .suffix(":1")
                            .speed(0.1),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "comp", "ratio");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Attack:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].comp_attack)
                            .range(0.1..=200.0)
                            .suffix(" ms")
                            .speed(0.5),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "comp", "attack");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Release:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].comp_release)
                            .range(10.0..=1000.0)
                            .suffix(" ms")
                            .speed(1.0),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "comp", "release");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Makeup:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].comp_makeup)
                            .range(0.0..=24.0)
                            .suffix(" dB")
                            .speed(0.2),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "comp", "makeup");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Knee:");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.channels[index].comp_knee)
                            .range(-24.0..=0.0)
                            .suffix(" dB")
                            .speed(0.2),
                    )
                    .changed()
                {
                    self.update_processing_param(ctx, index, "comp", "knee");
                }
            });
        });
    }

    /// Render EQ controls.
    fn render_eq_section(&mut self, ui: &mut Ui, ctx: &Context, index: usize) {
        ui.vertical(|ui| {
            let enabled = self.channels[index].eq_enabled;
            let header_color = if enabled {
                Color32::from_rgb(0, 100, 180)
            } else {
                Color32::GRAY
            };

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("EQ").color(header_color).strong());
                if ui
                    .checkbox(&mut self.channels[index].eq_enabled, "")
                    .changed()
                {
                    self.update_channel_property(ctx, index, "eq_enabled");
                }
            });

            ui.add_space(4.0);
            if !enabled {
                ui.disable();
            }

            let band_names = ["Low", "Lo-Mid", "Hi-Mid", "High"];

            ui.horizontal(|ui| {
                for (band, name) in band_names.iter().enumerate() {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(*name).small());

                        // Frequency
                        if ui
                            .add(
                                egui::DragValue::new(&mut self.channels[index].eq_bands[band].0)
                                    .range(20.0..=20000.0)
                                    .suffix(" Hz")
                                    .speed(10.0),
                            )
                            .changed()
                        {
                            self.update_eq_param(ctx, index, band, "freq");
                        }

                        // Gain
                        if ui
                            .add(
                                egui::DragValue::new(&mut self.channels[index].eq_bands[band].1)
                                    .range(-15.0..=15.0)
                                    .suffix(" dB")
                                    .speed(0.1),
                            )
                            .changed()
                        {
                            self.update_eq_param(ctx, index, band, "gain");
                        }

                        // Q
                        if ui
                            .add(
                                egui::DragValue::new(&mut self.channels[index].eq_bands[band].2)
                                    .range(0.1..=10.0)
                                    .prefix("Q ")
                                    .speed(0.05),
                            )
                            .changed()
                        {
                            self.update_eq_param(ctx, index, band, "q");
                        }
                    });

                    if band < 3 {
                        ui.add_space(8.0);
                    }
                }
            });
        });
    }

    /// Update a processing parameter (gate/comp).
    fn update_processing_param(
        &mut self,
        ctx: &Context,
        index: usize,
        processor: &str,
        param: &str,
    ) {
        if !self.live_updates {
            return;
        }

        let channel = &self.channels[index];

        let (element_suffix, gst_prop, value) = match (processor, param) {
            ("hpf", "enabled") => {
                let cutoff = if channel.hpf_enabled {
                    channel.hpf_freq
                } else {
                    1.0 // Bypass: set cutoff to minimum
                };
                (
                    format!("hpf_{}", index),
                    "cutoff".to_string(),
                    PropertyValue::Float(cutoff as f64),
                )
            }
            ("hpf", "freq") => (
                format!("hpf_{}", index),
                "cutoff".to_string(),
                PropertyValue::Float(channel.hpf_freq as f64),
            ),
            ("gate", "threshold") => (
                format!("gate_{}", index),
                "gt".to_string(),
                PropertyValue::Float(db_to_linear_f64(channel.gate_threshold as f64)),
            ),
            ("gate", "attack") => (
                format!("gate_{}", index),
                "at".to_string(),
                PropertyValue::Float(channel.gate_attack as f64),
            ),
            ("gate", "release") => (
                format!("gate_{}", index),
                "rt".to_string(),
                PropertyValue::Float(channel.gate_release as f64),
            ),
            // Note: LSP gate does not have a settable range property
            // ("rr" doesn't exist, "gr" is read-only reduction meter)
            ("gate", "range") => return,
            ("comp", "threshold") => (
                format!("comp_{}", index),
                "al".to_string(),
                PropertyValue::Float(db_to_linear_f64(channel.comp_threshold as f64)),
            ),
            ("comp", "ratio") => (
                format!("comp_{}", index),
                "cr".to_string(),
                PropertyValue::Float(channel.comp_ratio as f64),
            ),
            ("comp", "attack") => (
                format!("comp_{}", index),
                "at".to_string(),
                PropertyValue::Float(channel.comp_attack as f64),
            ),
            ("comp", "release") => (
                format!("comp_{}", index),
                "rt".to_string(),
                PropertyValue::Float(channel.comp_release as f64),
            ),
            ("comp", "makeup") => (
                format!("comp_{}", index),
                "mk".to_string(),
                PropertyValue::Float(db_to_linear_f64(channel.comp_makeup as f64)),
            ),
            ("comp", "knee") => (
                format!("comp_{}", index),
                "kn".to_string(),
                PropertyValue::Float(db_to_linear_f64(channel.comp_knee as f64).clamp(0.0631, 1.0)),
            ),
            _ => return,
        };

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let element_id = format!("{}:{}", self.block_id, element_suffix);
        let ctx = ctx.clone();

        crate::app::spawn_task(async move {
            let _ = api
                .update_element_property(&flow_id, &element_id, &gst_prop, value)
                .await;
            ctx.request_repaint();
        });
    }

    /// Update an EQ band parameter.
    fn update_eq_param(&mut self, ctx: &Context, index: usize, band: usize, param: &str) {
        if !self.live_updates {
            return;
        }

        let channel = &self.channels[index];
        let (freq, gain, q) = channel.eq_bands[band];

        let (gst_prop, value) = match param {
            "freq" => (format!("f-{}", band), PropertyValue::Float(freq as f64)),
            "gain" => (
                format!("g-{}", band),
                PropertyValue::Float(db_to_linear_f64(gain as f64)),
            ),
            "q" => (format!("q-{}", band), PropertyValue::Float(q as f64)),
            _ => return,
        };

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let element_id = format!("{}:eq_{}", self.block_id, index);
        let ctx = ctx.clone();

        crate::app::spawn_task(async move {
            let _ = api
                .update_element_property(&flow_id, &element_id, &gst_prop, value)
                .await;
            ctx.request_repaint();
        });
    }

    /// Update a main bus processing parameter via API.
    fn update_main_processing_param(&mut self, ctx: &Context, processor: &str, param: &str) {
        if !self.live_updates {
            return;
        }

        let (element_suffix, gst_prop, value) = match (processor, param) {
            ("comp", "enabled") => (
                "main_comp".to_string(),
                "enabled".to_string(),
                PropertyValue::Bool(self.main_comp_enabled),
            ),
            ("comp", "threshold") => (
                "main_comp".to_string(),
                "al".to_string(),
                PropertyValue::Float(db_to_linear_f64(self.main_comp_threshold as f64)),
            ),
            ("comp", "ratio") => (
                "main_comp".to_string(),
                "cr".to_string(),
                PropertyValue::Float(self.main_comp_ratio as f64),
            ),
            ("comp", "attack") => (
                "main_comp".to_string(),
                "at".to_string(),
                PropertyValue::Float(self.main_comp_attack as f64),
            ),
            ("comp", "release") => (
                "main_comp".to_string(),
                "rt".to_string(),
                PropertyValue::Float(self.main_comp_release as f64),
            ),
            ("comp", "makeup") => (
                "main_comp".to_string(),
                "mk".to_string(),
                PropertyValue::Float(db_to_linear_f64(self.main_comp_makeup as f64)),
            ),
            ("comp", "knee") => (
                "main_comp".to_string(),
                "kn".to_string(),
                PropertyValue::Float(
                    db_to_linear_f64(self.main_comp_knee as f64).clamp(0.0631, 1.0),
                ),
            ),
            ("eq", "enabled") => (
                "main_eq".to_string(),
                "enabled".to_string(),
                PropertyValue::Bool(self.main_eq_enabled),
            ),
            ("limiter", "enabled") => (
                "main_limiter".to_string(),
                "enabled".to_string(),
                PropertyValue::Bool(self.main_limiter_enabled),
            ),
            ("limiter", "threshold") => (
                "main_limiter".to_string(),
                "th".to_string(),
                PropertyValue::Float(db_to_linear_f64(self.main_limiter_threshold as f64)),
            ),
            _ => return,
        };

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let element_id = format!("{}:{}", self.block_id, element_suffix);
        let ctx = ctx.clone();

        crate::app::spawn_task(async move {
            let _ = api
                .update_element_property(&flow_id, &element_id, &gst_prop, value)
                .await;
            ctx.request_repaint();
        });
    }

    /// Update a main bus EQ band parameter via API.
    fn update_main_eq_param(&mut self, ctx: &Context, band: usize, param: &str) {
        if !self.live_updates {
            return;
        }

        let (freq, gain, q) = self.main_eq_bands[band];

        let (gst_prop, value) = match param {
            "freq" => (format!("f-{}", band), PropertyValue::Float(freq as f64)),
            "gain" => (
                format!("g-{}", band),
                PropertyValue::Float(db_to_linear_f64(gain as f64)),
            ),
            "q" => (format!("q-{}", band), PropertyValue::Float(q as f64)),
            _ => return,
        };

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let element_id = format!("{}:main_eq", self.block_id);
        let ctx = ctx.clone();

        crate::app::spawn_task(async move {
            let _ = api
                .update_element_property(&flow_id, &element_id, &gst_prop, value)
                .await;
            ctx.request_repaint();
        });
    }

    /// Handle keyboard shortcuts.
    fn handle_keyboard(&mut self, ui: &mut Ui, ctx: &Context) {
        // Ctrl+S = Save mixer state
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
            self.save_requested = true;
            self.status = "Saving mixer state...".to_string();
        }

        // Number keys 1-9, 0 for channel selection
        for (key, ch) in [
            (egui::Key::Num1, 0),
            (egui::Key::Num2, 1),
            (egui::Key::Num3, 2),
            (egui::Key::Num4, 3),
            (egui::Key::Num5, 4),
            (egui::Key::Num6, 5),
            (egui::Key::Num7, 6),
            (egui::Key::Num8, 7),
            (egui::Key::Num9, 8),
            (egui::Key::Num0, 9),
        ] {
            if ui.input(|i| i.key_pressed(key)) && ch < self.channels.len() {
                self.selection = Some(Selection::Channel(ch));
            }
        }

        // Extract selected channel index for channel-specific shortcuts
        let selected_ch = match self.selection {
            Some(Selection::Channel(ch)) => Some(ch),
            _ => None,
        };

        // M = Mute selected channel
        if ui.input(|i| i.key_pressed(egui::Key::M)) {
            if let Some(ch) = selected_ch {
                self.channels[ch].mute = !self.channels[ch].mute;
                self.update_channel_property(ctx, ch, "mute");
            }
        }

        // P = PFL selected channel
        if ui.input(|i| i.key_pressed(egui::Key::P)) {
            if let Some(ch) = selected_ch {
                self.channels[ch].pfl = !self.channels[ch].pfl;
            }
        }

        // Up/Down = Adjust fader
        if let Some(ch) = selected_ch {
            let fader_step = 0.05;
            if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                self.channels[ch].fader = (self.channels[ch].fader + fader_step).min(2.0);
                self.update_channel_property(ctx, ch, "fader");
            }
            if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                self.channels[ch].fader = (self.channels[ch].fader - fader_step).max(0.0);
                self.update_channel_property(ctx, ch, "fader");
            }

            // Left/Right = Adjust pan
            let pan_step = 0.1;
            if ui.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                self.channels[ch].pan = (self.channels[ch].pan - pan_step).max(-1.0);
                self.update_channel_property(ctx, ch, "pan");
            }
            if ui.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                self.channels[ch].pan = (self.channels[ch].pan + pan_step).min(1.0);
                self.update_channel_property(ctx, ch, "pan");
            }
        }
    }

    /// Update a channel property via API.
    fn update_channel_property(&mut self, ctx: &Context, index: usize, property: &str) {
        if !self.live_updates {
            return;
        }

        // Throttle updates
        if self.last_update.elapsed().as_millis() < 50 {
            return;
        }
        self.last_update = instant::Instant::now();

        let channel = &self.channels[index];

        // Map channel property to GStreamer element and property
        // The element_id format is "block_id:element_name"
        let (element_suffix, gst_prop, value) = match property {
            "gain" => (
                format!("gain_{}", index),
                "volume",
                PropertyValue::Float(db_to_linear_f64(channel.gain as f64)),
            ),
            "pan" => (
                format!("pan_{}", index),
                "panorama",
                PropertyValue::Float(channel.pan as f64),
            ),
            "fader" => {
                // If muted, set volume to 0, otherwise use fader value
                let effective_volume = if channel.mute {
                    0.0
                } else {
                    channel.fader as f64
                };
                (
                    format!("volume_{}", index),
                    "volume",
                    PropertyValue::Float(effective_volume),
                )
            }
            "mute" => {
                // Mute is implemented by setting volume to 0
                let effective_volume = if channel.mute {
                    0.0
                } else {
                    channel.fader as f64
                };
                (
                    format!("volume_{}", index),
                    "volume",
                    PropertyValue::Float(effective_volume),
                )
            }
            "gate_enabled" => (
                format!("gate_{}", index),
                "enabled",
                PropertyValue::Bool(channel.gate_enabled),
            ),
            "comp_enabled" => (
                format!("comp_{}", index),
                "enabled",
                PropertyValue::Bool(channel.comp_enabled),
            ),
            "eq_enabled" => (
                format!("eq_{}", index),
                "enabled",
                PropertyValue::Bool(channel.eq_enabled),
            ),
            "pfl" => (
                format!("pfl_volume_{}", index),
                "volume",
                PropertyValue::Float(if channel.pfl { 1.0 } else { 0.0 }),
            ),
            _ => return,
        };

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let element_id = format!("{}:{}", self.block_id, element_suffix);
        let gst_prop = gst_prop.to_string();
        let ctx = ctx.clone();

        crate::app::spawn_task(async move {
            let _ = api
                .update_element_property(&flow_id, &element_id, &gst_prop, value)
                .await;
            ctx.request_repaint();
        });
    }

    /// Update main fader via API.
    fn update_main_fader(&mut self, ctx: &Context) {
        if !self.live_updates {
            return;
        }

        // Throttle updates
        if self.last_update.elapsed().as_millis() < 50 {
            return;
        }
        self.last_update = instant::Instant::now();

        // Apply mute: if muted, send 0, otherwise send fader value
        let effective_volume = if self.main_mute {
            0.0
        } else {
            self.main_fader as f64
        };

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let element_id = format!("{}:main_volume", self.block_id);
        let value = PropertyValue::Float(effective_volume);
        let ctx = ctx.clone();

        crate::app::spawn_task(async move {
            let _ = api
                .update_element_property(&flow_id, &element_id, "volume", value)
                .await;
            ctx.request_repaint();
        });
    }

    /// Update main mute via API.
    fn update_main_mute(&mut self, ctx: &Context) {
        if !self.live_updates {
            return;
        }

        // Mute is implemented by setting volume to 0
        let effective_volume = if self.main_mute {
            0.0
        } else {
            self.main_fader as f64
        };

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let element_id = format!("{}:main_volume", self.block_id);
        let value = PropertyValue::Float(effective_volume);
        let ctx = ctx.clone();

        crate::app::spawn_task(async move {
            let _ = api
                .update_element_property(&flow_id, &element_id, "volume", value)
                .await;
            ctx.request_repaint();
        });
    }

    /// Update aux send level via API.
    fn update_aux_send(&mut self, ctx: &Context, ch_idx: usize, aux_idx: usize) {
        if !self.live_updates {
            return;
        }

        let level = self.channels[ch_idx].aux_sends[aux_idx] as f64;

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let element_id = format!("{}:aux_send_{}_{}", self.block_id, ch_idx, aux_idx);
        let value = PropertyValue::Float(level);
        let ctx = ctx.clone();

        crate::app::spawn_task(async move {
            let _ = api
                .update_element_property(&flow_id, &element_id, "volume", value)
                .await;
            ctx.request_repaint();
        });
    }

    /// Update channel routing via API.
    /// Routing is implemented using volume elements - all destinations are always
    /// connected, we just set volume to 1.0 for active route and 0.0 for inactive.
    fn update_routing(&mut self, ctx: &Context, ch_idx: usize) {
        if !self.live_updates {
            return;
        }

        let to_main = self.channels[ch_idx].to_main;
        let to_grp = self.channels[ch_idx].to_grp;
        let num_groups = self.num_groups;

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let block_id = self.block_id.clone();
        let ctx = ctx.clone();

        // Update to_main volume
        let to_main_vol = if to_main { 1.0 } else { 0.0 };
        let to_main_id = format!("{}:to_main_vol_{}", block_id, ch_idx);

        let api_clone = api.clone();
        let ctx_clone = ctx.clone();
        crate::app::spawn_task(async move {
            let _ = api_clone
                .update_element_property(
                    &flow_id,
                    &to_main_id,
                    "volume",
                    PropertyValue::Float(to_main_vol),
                )
                .await;
            ctx_clone.request_repaint();
        });

        // Update each group route volume
        for (sg, &enabled) in to_grp.iter().enumerate().take(num_groups) {
            let route_sg_vol = if enabled { 1.0 } else { 0.0 };
            let to_grp_id = format!("{}:to_grp{}_vol_{}", block_id, sg, ch_idx);

            let api_clone = api.clone();
            let flow_id_clone = flow_id;
            let ctx_clone = ctx.clone();
            crate::app::spawn_task(async move {
                let _ = api_clone
                    .update_element_property(
                        &flow_id_clone,
                        &to_grp_id,
                        "volume",
                        PropertyValue::Float(route_sg_vol),
                    )
                    .await;
                ctx_clone.request_repaint();
            });
        }

        // Build routing description for logging
        let mut routes = Vec::new();
        if to_main {
            routes.push("Main".to_string());
        }
        for (sg, &enabled) in to_grp.iter().enumerate().take(num_groups) {
            if enabled {
                routes.push(format!("GRP{}", sg + 1));
            }
        }
        let routes_str = if routes.is_empty() {
            "None".to_string()
        } else {
            routes.join(", ")
        };
        tracing::info!("Routing updated: Ch {} -> {}", ch_idx + 1, routes_str);
    }

    /// Update group fader via API.
    fn update_group_fader(&mut self, ctx: &Context, sg_idx: usize) {
        if !self.live_updates {
            return;
        }

        // Throttle updates
        if self.last_update.elapsed().as_millis() < 50 {
            return;
        }
        self.last_update = instant::Instant::now();

        let mute = self.groups[sg_idx].mute;
        let effective_volume = if mute {
            0.0
        } else {
            self.groups[sg_idx].fader as f64
        };

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let element_id = format!("{}:group{}_volume", self.block_id, sg_idx);
        let value = PropertyValue::Float(effective_volume);
        let ctx = ctx.clone();

        crate::app::spawn_task(async move {
            let _ = api
                .update_element_property(&flow_id, &element_id, "volume", value)
                .await;
            ctx.request_repaint();
        });
    }

    /// Update group mute via API.
    fn update_group_mute(&mut self, ctx: &Context, sg_idx: usize) {
        if !self.live_updates {
            return;
        }

        let mute = self.groups[sg_idx].mute;
        let effective_volume = if mute {
            0.0
        } else {
            self.groups[sg_idx].fader as f64
        };

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let element_id = format!("{}:group{}_volume", self.block_id, sg_idx);
        let value = PropertyValue::Float(effective_volume);
        let ctx = ctx.clone();

        crate::app::spawn_task(async move {
            let _ = api
                .update_element_property(&flow_id, &element_id, "volume", value)
                .await;
            ctx.request_repaint();
        });
    }

    /// Update aux master fader via API.
    fn update_aux_master_fader(&mut self, ctx: &Context, aux_idx: usize) {
        if !self.live_updates {
            return;
        }

        // Throttle updates
        if self.last_update.elapsed().as_millis() < 50 {
            return;
        }
        self.last_update = instant::Instant::now();

        let mute = self.aux_masters[aux_idx].mute;
        let effective_volume = if mute {
            0.0
        } else {
            self.aux_masters[aux_idx].fader as f64
        };

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let element_id = format!("{}:aux{}_volume", self.block_id, aux_idx);
        let value = PropertyValue::Float(effective_volume);
        let ctx = ctx.clone();

        crate::app::spawn_task(async move {
            let _ = api
                .update_element_property(&flow_id, &element_id, "volume", value)
                .await;
            ctx.request_repaint();
        });
    }

    /// Update aux master mute via API.
    fn update_aux_master_mute(&mut self, ctx: &Context, aux_idx: usize) {
        if !self.live_updates {
            return;
        }

        let mute = self.aux_masters[aux_idx].mute;
        let effective_volume = if mute {
            0.0
        } else {
            self.aux_masters[aux_idx].fader as f64
        };

        let api = self.api.clone();
        let flow_id = self.flow_id;
        let element_id = format!("{}:aux{}_volume", self.block_id, aux_idx);
        let value = PropertyValue::Float(effective_volume);
        let ctx = ctx.clone();

        crate::app::spawn_task(async move {
            let _ = api
                .update_element_property(&flow_id, &element_id, "volume", value)
                .await;
            ctx.request_repaint();
        });
    }
}

/// Format a linear fader value as dB string.
fn format_db(linear: f32) -> String {
    if linear <= 0.001 {
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
fn format_pan(pan: f32) -> String {
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
fn db_to_y(db: f32, y_min: f32, y_max: f32) -> f32 {
    let normalized = ((db - (-60.0)) / 66.0).clamp(0.0, 1.0);
    let margin = 5.0;
    let usable = (y_max - y_min) - margin * 2.0;
    y_max - margin - normalized * usable
}

/// Convert dB to linear level (0.0-1.0).
fn db_to_level(db: f64) -> f32 {
    let min_db = -60.0;
    let max_db = 6.0;
    ((db - min_db) / (max_db - min_db)).clamp(0.0, 1.0) as f32
}

/// Get color for a level value.
fn level_to_color(level: f32) -> Color32 {
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
fn db_to_linear_f64(db: f64) -> f64 {
    10.0_f64.powf(db / 20.0)
}

/// Convert dB to linear scale (f32).
fn db_to_linear_f32(db: f32) -> f32 {
    if db <= -60.0 {
        0.0
    } else {
        10.0_f32.powf(db / 20.0)
    }
}

/// Convert linear to dB scale.
fn linear_to_db(linear: f64) -> f64 {
    if linear <= 0.0001 {
        -60.0
    } else {
        20.0 * linear.log10()
    }
}

/// Convert a linear level (0.0–2.0) to a knob arc position (0.0–1.0).
///
/// dB-scaled: first half of arc = -60..0 dB, second half = 0..+6 dB.
/// This puts unity (0 dB, linear 1.0) at the center of the arc (12 o'clock).
fn knob_linear_to_normalized(linear: f32) -> f32 {
    if linear <= 0.001 {
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
