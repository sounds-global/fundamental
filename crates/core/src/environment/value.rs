//! The [`Environment`] enum and its security/logging policy accessors.

use super::{EnvironmentPosture, ParseEnvironmentError};

#[cfg(not(feature = "std"))]
use alloc::string::String;
use core::fmt;
use core::str::FromStr;

/// The deployment environment.
///
/// # Examples
///
/// ```
/// use fundamental_core::Environment;
///
/// // Default is Development
/// let env = Environment::default();
/// assert!(env.is_development());
///
/// // Parse from string
/// let prod: Environment = "production".parse().unwrap();
/// assert!(prod.is_production());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Environment {
    /// Local development environment.
    #[default]
    Development,

    /// Testing/CI environment.
    Test,

    /// Pre-production staging environment.
    Staging,

    /// Production environment.
    Production,
}

impl Environment {
    /// Environment variable name for detection.
    pub const ENV_VAR: &'static str = "FUNDAMENTAL_ENV";

    /// The custom environment name a preview deployment runs under: `FUNDAMENTAL_ENV=preview`.
    pub const PREVIEW: &'static str = "preview";

    /// Reads `FUNDAMENTAL_ENV`, answering `Development` when it is unset or unrecognised.
    ///
    /// Fail-open: use `fundamental_config::resolve_environment`, which refuses to guess in a release build.
    #[cfg(feature = "std")]
    #[deprecated(
        note = "fail-open: resolve with fundamental_config::resolve_environment, which refuses an unset or undeclared environment in a release build"
    )]
    #[must_use]
    pub fn detect() -> Self {
        std::env::var(Self::ENV_VAR)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_default()
    }

    /// Returns the canonical name of this environment.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Test => "test",
            Self::Staging => "staging",
            Self::Production => "production",
        }
    }

    /// The posture this environment is held to: deployed for staging and production.
    #[must_use]
    pub const fn posture(&self) -> EnvironmentPosture {
        if self.is_deployed() {
            EnvironmentPosture::Deployed
        } else {
            EnvironmentPosture::Development
        }
    }

    /// Returns whether this is the development environment.
    #[must_use]
    pub const fn is_development(&self) -> bool {
        matches!(self, Self::Development)
    }

    /// Returns whether this is the test environment.
    #[must_use]
    pub const fn is_test(&self) -> bool {
        matches!(self, Self::Test)
    }

    /// Returns whether this is the staging environment.
    #[must_use]
    pub const fn is_staging(&self) -> bool {
        matches!(self, Self::Staging)
    }

    /// Returns whether this is the production environment.
    #[must_use]
    pub const fn is_production(&self) -> bool {
        matches!(self, Self::Production)
    }

    /// Returns whether this is a local environment (development or test).
    #[must_use]
    pub const fn is_local(&self) -> bool {
        matches!(self, Self::Development | Self::Test)
    }

    /// Returns whether this is a deployed environment (staging or production).
    #[must_use]
    pub const fn is_deployed(&self) -> bool {
        matches!(self, Self::Staging | Self::Production)
    }

    /// Returns whether insecure TLS (disabled or unverified) is allowed.
    ///
    /// Only for local environments in debug builds; a release build always requires TLS.
    #[must_use]
    pub const fn allows_insecure_tls(&self) -> bool {
        cfg!(debug_assertions) && self.is_local()
    }

    /// Returns whether file-based secrets (.env files) are allowed.
    #[must_use]
    pub const fn allows_file_secrets(&self) -> bool {
        self.is_local()
    }

    /// Returns whether a secret manager is required.
    /// Returns whether a secret manager is required (production only).
    #[must_use]
    pub const fn requires_secret_manager(&self) -> bool {
        matches!(self, Self::Production)
    }

    /// Returns whether this environment is protected.
    #[must_use]
    pub const fn is_protected(&self) -> bool {
        matches!(self, Self::Production)
    }

    /// Returns the recommended log level for this environment.
    #[must_use]
    pub const fn default_log_level(&self) -> &'static str {
        match self {
            Self::Development | Self::Test => "debug",
            Self::Staging => "info",
            Self::Production => "warn",
        }
    }

    /// Returns the recommended log format for this environment.
    #[must_use]
    pub const fn default_log_format(&self) -> &'static str {
        match self {
            Self::Development | Self::Test => "pretty",
            Self::Staging | Self::Production => "json",
        }
    }
}

impl fmt::Display for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Environment {
    type Err = ParseEnvironmentError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "development" | "dev" | "local" => Ok(Self::Development),
            "test" | "testing" | "ci" => Ok(Self::Test),
            "staging" | "stage" | "preprod" => Ok(Self::Staging),
            "production" | "prod" => Ok(Self::Production),
            _ => Err(ParseEnvironmentError::new(s)),
        }
    }
}

impl serde::Serialize for Environment {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for Environment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}
