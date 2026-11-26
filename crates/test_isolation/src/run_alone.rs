//! [`RunAlone`], the tests a project runs with nothing beside them.

use std::collections::BTreeSet;

use crate::IsolationError;

/// The tests and test binaries a project runs with nothing beside them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunAlone {
    tests: BTreeSet<String>,
    binaries: BTreeSet<String>,
}

impl RunAlone {
    /// The run-alone tests a `.config/nextest.toml` names.
    ///
    /// # Errors
    ///
    /// [`IsolationError::RunAloneUnreadable`] when the file is not TOML or a filter term has another form.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub fn from_nextest(config: &str) -> Result<Self, IsolationError> {
        let unreadable = |message: String| IsolationError::RunAloneUnreadable { message };
        let config: toml::Table = config
            .parse()
            .map_err(|error: toml::de::Error| unreadable(error.to_string()))?;
        let overrides = config
            .get("profile")
            .and_then(|profile| profile.get("default"))
            .and_then(|default| default.get("overrides"))
            .and_then(toml::Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        let mut alone = Self::default();
        for filter in overrides
            .iter()
            .filter(|o| {
                o.get("threads-required").and_then(toml::Value::as_str) == Some("num-test-threads")
            })
            .filter_map(|o| o.get("filter").and_then(toml::Value::as_str))
        {
            for term in filter.split('|').map(str::trim) {
                if let Some(name) = term
                    .strip_prefix("test(=")
                    .and_then(|t| t.strip_suffix(')'))
                {
                    // The check judges functions, so `module::function` is held as its function.
                    let function = name.rsplit("::").next().unwrap_or(name);
                    alone.tests.insert(function.to_owned());
                } else if let Some(name) = term
                    .strip_prefix("binary(")
                    .and_then(|t| t.strip_suffix(')'))
                    .or_else(|| {
                        term.strip_prefix("test(/^")
                            .and_then(|t| t.strip_suffix("::/)"))
                    })
                    .filter(|name| name.chars().all(|c| c.is_alphanumeric() || c == '_'))
                {
                    alone.binaries.insert(name.to_owned());
                } else {
                    return Err(unreadable(format!(
                        "`{term}` is none of `test(=name)`, `binary(name)` or `test(/^module::/)`"
                    )));
                }
            }
        }
        Ok(alone)
    }

    /// Whether the test function `name` runs alone.
    #[must_use]
    pub fn holds_test(&self, name: &str) -> bool {
        self.tests.contains(name)
    }

    /// Whether every test of the binary, or the test module, `name` runs alone.
    #[must_use]
    pub fn holds_binary(&self, name: &str) -> bool {
        self.binaries.contains(name)
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::RunAlone;

    const CONFIG: &str = r#"
[profile.default]
test-threads = 8

[[profile.default.overrides]]
filter = 'binary(leases) | test(=a_review_falls_due) | test(/^drains::/) | test(=signing_in::one_address)'
threads-required = "num-test-threads"

[[profile.default.overrides]]
filter = 'test(=slow_but_shared)'
slow-timeout = "60s"
"#;

    #[test]
    fn the_overrides_that_take_every_thread_name_what_runs_alone() {
        let alone = RunAlone::from_nextest(CONFIG).expect("a readable list");
        assert!(alone.holds_binary("leases"));
        assert!(
            alone.holds_binary("drains"),
            "a module of the one test binary"
        );
        assert!(alone.holds_test("a_review_falls_due"));
        assert!(
            alone.holds_test("one_address"),
            "a test named by its module"
        );
        assert!(
            !alone.holds_test("slow_but_shared"),
            "another override is not alone"
        );
    }

    #[test]
    fn a_term_the_check_cannot_honour_is_refused() {
        let refused = RunAlone::from_nextest(
            "[[profile.default.overrides]]\nfilter = 'test(~review)'\nthreads-required = \"num-test-threads\"\n",
        )
        .expect_err("a substring match cannot be honoured");
        assert_eq!(refused.error_code(), "ISO_103");
    }

    #[test]
    fn no_override_is_nothing_alone() {
        let alone = RunAlone::from_nextest("[profile.default]\n").expect("a readable list");
        assert_eq!(alone, RunAlone::default());
    }
}
