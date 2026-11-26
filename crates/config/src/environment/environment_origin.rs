//! [`EnvironmentOrigin`]: where a process's environment came from, before it is resolved.

use fundamental_core::Environment;
use fundamental_core::env::EnvVars;

use std::fmt;

/// Where a process's environment came from, before [`resolve_environment`] decides it.
///
/// [`resolve_environment`]: crate::resolve_environment
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvironmentOrigin {
    /// `FUNDAMENTAL_ENV` is unset, or blank.
    Unset,
    /// `FUNDAMENTAL_ENV` names this, trimmed: a built-in, an alias, or a custom name.
    Variable(String),
    /// Code chose the environment, and optionally the `[environments.*]` table read for it.
    Chosen {
        /// The environment whose rules apply.
        environment: Environment,
        /// A custom table to read overrides from, instead of the environment's own.
        name: Option<String>,
    },
}

impl EnvironmentOrigin {
    /// Reads `FUNDAMENTAL_ENV` from `vars`, trimmed; only whitespace counts as unset.
    #[must_use]
    pub fn from_vars(vars: &EnvVars) -> Self {
        match vars.get(Environment::ENV_VAR) {
            Some(raw) if !raw.trim().is_empty() => Self::Variable(raw.trim().to_owned()),
            _ => Self::Unset,
        }
    }

    /// A stable one-word label for logs: `unset`, `variable` or `code`.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Unset => "unset",
            Self::Variable(_) => "variable",
            Self::Chosen { .. } => "code",
        }
    }
}

impl fmt::Display for EnvironmentOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_or_absent_variable_is_unset() {
        assert_eq!(
            EnvironmentOrigin::from_vars(&EnvVars::none()),
            EnvironmentOrigin::Unset
        );
        for blank in ["", " ", "\n", "\t \n"] {
            let vars = EnvVars::fixed([(Environment::ENV_VAR, blank)]);
            assert_eq!(
                EnvironmentOrigin::from_vars(&vars),
                EnvironmentOrigin::Unset
            );
        }
    }

    #[test]
    fn a_value_is_trimmed_before_it_is_read() {
        let vars = EnvVars::fixed([(Environment::ENV_VAR, " production\n")]);
        assert_eq!(
            EnvironmentOrigin::from_vars(&vars),
            EnvironmentOrigin::Variable("production".to_owned())
        );
    }
}
