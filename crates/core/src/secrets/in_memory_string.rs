//! In-memory secret string alias.

#[cfg(not(feature = "std"))]
use alloc::string::String;

use super::InMemorySecret;

/// An in-memory secret string value.
///
/// # Example
///
/// ```
/// use fundamental_core::secrets::{InMemorySecret, InMemorySecretString};
///
/// let vault_token: InMemorySecretString = InMemorySecret::new("s.hvs.TOKEN".to_string());
/// assert_eq!(format!("{vault_token:?}"), "[REDACTED]");
/// assert_eq!(vault_token.expose().map(String::as_str), Some("s.hvs.TOKEN"));
/// ```
pub type InMemorySecretString = InMemorySecret<String>;
