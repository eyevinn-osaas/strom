//! Statistics collection module for running pipelines.
//!
//! This module provides functions to collect runtime statistics from
//! GStreamer elements within running pipelines, particularly for
//! RTP/AES67 related blocks.

pub mod collector;
pub mod rtp;

pub use collector::StatsCollector;
pub use rtp::collect_rtp_jitterbuffer_stats;
