//! [`EnvironmentPosture`]: which set of rules an environment is held to.

use super::{Environment, ParsePostureError};

use core::fmt;
use core::str::FromStr;

/// Which set of rules an environment is held to: deployed, or a developer's own machine.
///
/// # Examples
///
/// ```
/// use fundamental_core::{Environment, EnvironmentPosture};
///
/// assert_eq!(Environment::Staging.posture(), EnvironmentPosture::Deployed);
/// assert_eq!("deployed".parse(), Ok(EnvironmentPosture::Deployed));
/// assert_eq!(EnvironmentPosture::Deployed.enforced_as(), Environment::Production);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvironmentPosture {
    /// Every deployed-only rule applies: TLS, the secret store, credential rules.
    Deployed,
    /// The rules of a developer's machine, where each of those may be relaxed.
    Development,
}

impl EnvironmentPosture {
    /// The manifest key a custom environment declares its posture under.
    pub const KEY: &'static str = "posture";

    /// The values [`KEY`](Self::KEY) accepts, in the order an error lists them.
    pub const ACCEPTED: &'static str = "\"deployed\" or \"development\"";

    /// The value as the manifest spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Deployed => "deployed",
            Self::Development => "development",
        }
    }

    /// The built-in environment whose rules this posture enforces.
    #[must_use]
    pub const fn enforced_as(self) -> Environment {
        match self {
            Self::Deployed => Environment::Production,
            Self::Development => Environment::Development,
        }
    }
}

impl fmt::Display for EnvironmentPosture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EnvironmentPosture {
    type Err = ParsePostureError;

    // Exact spelling only: this value decides whether security rules apply.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "deployed" => Ok(Self::Deployed),
            "development" => Ok(Self::Development),
            _ => Err(ParsePostureError::new(value)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_environment_has_the_posture_its_rules_imply() {
        for environment in [Environment::Staging, Environment::Production] {
            assert_eq!(environment.posture(), EnvironmentPosture::Deployed);
            assert!(environment.is_deployed());
        }
        for environment in [Environment::Development, Environment::Test] {
            assert_eq!(environment.posture(), EnvironmentPosture::Development);
            assert!(environment.is_local());
        }
    }

    #[test]
    fn a_posture_enforces_a_builtin_of_the_same_posture() {
        for posture in [
            EnvironmentPosture::Deployed,
            EnvironmentPosture::Development,
        ] {
            assert_eq!(posture.enforced_as().posture(), posture);
            assert_eq!(posture.as_str().parse(), Ok(posture));
        }
        assert_eq!(
            EnvironmentPosture::Deployed.enforced_as(),
            Environment::Production
        );
    }

    #[test]
    fn a_near_miss_is_refused_rather_than_interpreted() {
        for value in ["Deployed", "deploy", "production", " deployed", "dev", ""] {
            assert_eq!(
                value.parse::<EnvironmentPosture>(),
                Err(ParsePostureError::new(value))
            );
        }
    }
}
