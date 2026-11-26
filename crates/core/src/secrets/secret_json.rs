//! Secret JSON alias.

use super::Secret;

/// A secret structured value, encrypted at rest as JSON.
///
/// The inner `T` must be `Serialize` and `DeserializeOwned`; a bare `Secret<T>` does not compile as a model field.
///
/// # Example
///
/// ```
/// use fundamental_core::secrets::{Secret, SecretJson};
///
/// let codes: SecretJson<Vec<String>> =
///     Secret::new(vec!["a1b2".to_string(), "c3d4".to_string()]);
///
/// // Redacted everywhere it could leak.
/// assert_eq!(format!("{codes:?}"), "[REDACTED]");
///
/// // Read the structured value explicitly.
/// assert_eq!(codes.expose().len(), 2);
/// ```
pub type SecretJson<T> = Secret<T>;
