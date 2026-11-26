//! [`ConfigResult`], `Result<T, ConfigError>`.

use super::ConfigError;

/// Result type for configuration operations.
pub type ConfigResult<T> = Result<T, ConfigError>;
