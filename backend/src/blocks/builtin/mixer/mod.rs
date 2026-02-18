//! Stereo Mixer block - a digital mixing console for audio.
//!
//! This block provides a mixer similar to digital consoles like Behringer X32:
//! - Configurable number of input channels (1-32)
//! - Per-channel: input gain, gate, compressor, 4-band parametric EQ, pan, fader, mute
//! - Aux sends (0-4 configurable aux buses, switchable pre/post fader)
//! - Groups (0-4 configurable, with output pads)
//! - PFL (Pre-Fader Listen) bus with master level
//! - Main stereo bus with compressor, EQ, limiter, and master fader
//! - Per-channel and bus metering
//!
//! Pipeline structure per channel:
//! ```text
//! input_N → audioconvert → capsfilter(F32LE) → gain → hpf → gate → compressor → EQ →
//!           pre_fader_tee → audiopanorama_N → volume_N → post_fader_tee →
//!           level_N → [group or main audiomixer]
//!
//! (pre_fader_tee | post_fader_tee) → solo_volume_N → solo_queue_N → pfl_mixer
//!   (source depends on solo_mode: pfl=pre-fader, afl=post-fader)
//! (pre_fader_tee | post_fader_tee) → aux_send_N_M → aux_queue_N_M → aux_M_mixer
//! ```
//!
//! Main bus: audiomixer → main_comp → main_eq → main_limiter → main_volume → main_level → main_out_tee
//!
//! All output buses terminate in a tee with allow-not-linked=true, so unconnected
//! output pads don't cause NOT_LINKED flow errors. Audiomixer elements use
//! force-live=true so unconnected input pads don't stall the pipeline.
//!
//! Processing uses LSP LV2 plugins when available. Falls back to identity passthrough
//! when LV2 plugins are not installed.

mod builder;
mod definition;
mod elements;
mod metering;
mod properties;
#[cfg(test)]
mod tests;

/// Maximum number of input channels
const MAX_CHANNELS: usize = 32;
/// Default number of channels
const DEFAULT_CHANNELS: usize = 8;
/// Maximum number of aux buses
const MAX_AUX_BUSES: usize = 4;
/// Maximum number of groups
const MAX_GROUPS: usize = 4;

// Public API
pub use builder::MixerBuilder;
pub use definition::get_blocks;
pub use properties::translate_property_for_element;

// Crate-internal re-imports (accessible via super::* in tests)
#[cfg(test)]
use definition::mixer_definition;
#[cfg(test)]
use elements::*;
#[cfg(test)]
use metering::*;
#[cfg(test)]
use properties::*;
