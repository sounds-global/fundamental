//! [`EvalLimits`]: the bounds that keep a single check finite.

/// The safety bounds applied to every check evaluation.
///
/// Hitting a limit is an error (`AUTHZ_010`/`AUTHZ_011`), never a silent denial.
#[derive(Debug, Clone, Copy)]
pub struct EvalLimits {
    /// Maximum userset-rewrite recursion depth before the check errors.
    pub max_depth: usize,
    /// Maximum number of tuple reads a single check may perform.
    pub max_reads: usize,
    /// Maximum rows read from any one enumeration (usersets / tuple-to-userset).
    pub page_limit: usize,
    /// Whether independent branches of a check are evaluated concurrently.
    ///
    /// On by default; `false` gives a sequential walk with reproducible read order and the same
    /// decision.
    pub parallel_fanout: bool,
}

impl Default for EvalLimits {
    fn default() -> Self {
        Self {
            max_depth: 32,
            max_reads: 1000,
            page_limit: 1000,
            parallel_fanout: true,
        }
    }
}

#[cfg(feature = "config")]
impl EvalLimits {
    /// Builds the engine's limits from the `[authz]` configuration section.
    ///
    /// The default consistency is per request: map it with
    /// `Consistency::from(config.authz.default_consistency)`.
    #[must_use]
    pub fn from_settings(settings: &fundamental_config::settings::AuthzSettings) -> Self {
        Self {
            max_depth: settings.max_eval_depth,
            max_reads: settings.max_reads,
            page_limit: settings.page_limit,
            parallel_fanout: settings.parallel_fanout,
        }
    }
}

#[cfg(all(test, feature = "config"))]
mod tests {
    use super::*;
    use fundamental_config::settings::AuthzSettings;

    #[test]
    fn maps_every_bound_and_the_fanout_toggle() {
        let settings = AuthzSettings {
            enabled: true,
            max_eval_depth: 8,
            max_reads: 50,
            page_limit: 25,
            parallel_fanout: false,
            ..AuthzSettings::default()
        };
        let limits = EvalLimits::from_settings(&settings);
        assert_eq!(limits.max_depth, 8);
        assert_eq!(limits.max_reads, 50);
        assert_eq!(limits.page_limit, 25);
        assert!(!limits.parallel_fanout);
    }

    #[test]
    fn the_default_section_yields_the_engine_defaults() {
        let limits = EvalLimits::from_settings(&AuthzSettings::default());
        let default = EvalLimits::default();
        assert_eq!(limits.max_depth, default.max_depth);
        assert_eq!(limits.max_reads, default.max_reads);
        assert_eq!(limits.page_limit, default.page_limit);
        assert_eq!(limits.parallel_fanout, default.parallel_fanout);
    }
}
