//! Per-domain configuration ([`DomainsConfig`], [`DomainConfig`]).

use std::collections::HashMap;

use super::DatabaseSettings;

/// All domain configurations (for `type = "domain"` projects), each its own database boundary.
#[derive(Debug, Clone, Default)]
pub struct DomainsConfig {
    /// Domain configurations, keyed by domain name.
    pub domains: HashMap<String, DomainConfig>,
}

/// Configuration for a single domain within a domain project.
#[derive(Debug, Clone, Default)]
pub struct DomainConfig {
    /// Cargo feature flag that gates this domain's models.
    pub feature: String,
    /// Database configuration for this domain.
    pub database: DatabaseSettings,
}
