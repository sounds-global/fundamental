//! Authorization engine settings ([`AuthzSettings`]).

use serde::{Deserialize, Serialize};

/// Configuration for the authorization check engine; the bounds keep any one check finite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthzSettings {
    /// Whether authorization is enabled; set when an `[authz]` section is present.
    pub enabled: bool,
    /// Maximum userset-rewrite recursion depth before a check errors.
    pub max_eval_depth: usize,
    /// Maximum number of tuple reads a single check may perform.
    pub max_reads: usize,
    /// Maximum rows read from any one enumeration (usersets / tuple-to-userset).
    pub page_limit: usize,
    /// Whether the independent branches of a check (union alternatives,
    /// tuple-to-userset hops, stored group grants) evaluate concurrently.
    pub parallel_fanout: bool,
    /// The default read consistency when a request does not pin a freshness token.
    pub default_consistency: AuthzConsistency,
}

/// The read consistency a check requests by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthzConsistency {
    /// Prefer availability and low latency, accepting bounded staleness. The default.
    #[default]
    MinimizeLatency,
    /// Read the freshest consistent snapshot — the strongest guarantee, at a cost.
    FullConsistency,
}

impl Default for AuthzSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            max_eval_depth: 32,
            max_reads: 1000,
            page_limit: 1000,
            parallel_fanout: true,
            default_consistency: AuthzConsistency::MinimizeLatency,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_disabled_with_the_engine_default_bounds() {
        let settings = AuthzSettings::default();
        assert!(!settings.enabled);
        assert_eq!(settings.max_eval_depth, 32);
        assert_eq!(settings.max_reads, 1000);
        assert_eq!(settings.page_limit, 1000);
        assert!(settings.parallel_fanout);
        assert_eq!(
            settings.default_consistency,
            AuthzConsistency::MinimizeLatency
        );
    }

    #[test]
    fn deserializes_a_partial_section_over_defaults() {
        let settings: AuthzSettings = toml::from_str(
            r#"
            max_reads = 5000
            default_consistency = "full_consistency"
            "#,
        )
        .expect("valid authz settings");
        assert_eq!(settings.max_reads, 5000);
        assert_eq!(
            settings.default_consistency,
            AuthzConsistency::FullConsistency
        );
        assert_eq!(settings.max_eval_depth, 32);
        assert!(settings.parallel_fanout);
    }
}
