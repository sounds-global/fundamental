//! Top-level app registry ([`AppsConfig`]).

use super::AppConfig;

use std::collections::HashMap;

/// All app configurations, keyed by app name.
#[derive(Debug, Clone, Default)]
pub struct AppsConfig {
    /// Apps, keyed by app name.
    pub apps: HashMap<String, AppConfig>,
}
