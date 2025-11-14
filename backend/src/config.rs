//! Configuration management.

use std::env;

/// Application configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Port to listen on
    pub port: u16,
    /// Path to flows storage file
    #[allow(dead_code)]
    pub flows_path: String,
}

impl Config {
    /// Load configuration from environment variables.
    pub fn from_env() -> Self {
        Self {
            port: env::var("STROM_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3000),
            flows_path: env::var("STROM_FLOWS_PATH").unwrap_or_else(|_| "flows.json".to_string()),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 3000,
            flows_path: "flows.json".to_string(),
        }
    }
}
