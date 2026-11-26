//! Secret string alias.

#[cfg(not(feature = "std"))]
use alloc::string::String;

use super::Secret;

/// A secret string value.
///
/// # Example
///
/// ```
/// use fundamental_core::secrets::SecretString;
/// use fundamental_core::secrets::Secret;
///
/// fn authenticate(password: SecretString) {
///     // Safe to use in logs - shows [REDACTED]
///     println!("Authenticating with password: {:?}", password);
///
///     // Actually use the password
///     let actual_password = password.expose();
///     // ... verify password ...
/// }
///
/// let password = Secret::new("secret123".to_string());
/// authenticate(password);
/// ```
pub type SecretString = Secret<String>;
