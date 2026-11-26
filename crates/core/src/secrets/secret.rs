//! Generic secret wrapper.

use core::fmt;

/// A wrapper type that protects sensitive data from accidental exposure.
///
/// It implements no `PartialEq`: compare by exposing and choosing a constant-time comparison.
///
/// ```
/// use fundamental_core::secrets::Secret;
/// use subtle::ConstantTimeEq;
///
/// let held = Secret::new("s3cret".to_string());
/// let offered = Secret::new("s3cret".to_string());
///
/// let same: bool = held.expose().as_bytes().ct_eq(offered.expose().as_bytes()).into();
/// assert!(same);
/// ```
///
/// `==` on two secrets does not compile:
///
/// ```compile_fail
/// use fundamental_core::secrets::Secret;
///
/// let a = Secret::new("same".to_string());
/// let b = Secret::new("same".to_string());
/// // error[E0369]: binary operation `==` cannot be applied to type `Secret<String>`
/// let _ = a == b;
/// ```
///
/// # Example
///
/// ```
/// use fundamental_core::secrets::Secret;
///
/// let api_key = Secret::new("sk-1234567890".to_string());
///
/// // Safe to log - shows [REDACTED]
/// println!("API Key: {:?}", api_key);
///
/// // Explicit access when needed
/// let actual = api_key.expose();
/// ```
pub struct Secret<T>(T);

impl<T> Secret<T> {
    /// Creates a new secret from the given value.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// Exposes the secret value by reference.
    #[must_use]
    pub fn expose(&self) -> &T {
        &self.0
    }

    /// Exposes the secret value by mutable reference.
    pub fn expose_mut(&mut self) -> &mut T {
        &mut self.0
    }

    /// Consumes the secret and returns the inner value.
    #[must_use]
    pub fn expose_owned(self) -> T {
        self.0
    }

    /// Maps the secret value to a new type.
    pub fn map<U, F>(self, f: F) -> Secret<U>
    where
        F: FnOnce(T) -> U,
    {
        Secret(f(self.0))
    }
}

impl<T: Clone> Clone for Secret<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl<T: Default> Default for Secret<T> {
    fn default() -> Self {
        Self(T::default())
    }
}

// No `PartialEq`, deliberately: `String`'s early-return `==` is a timing oracle.
// To compare two secrets, expose both and use `subtle::ConstantTimeEq`.

// Serialization redacts; deliberate persistence goes through `expose*`.
impl<T> serde::Serialize for Secret<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str("[REDACTED]")
    }
}

impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for Secret<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        T::deserialize(deserializer).map(Secret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_redacts_and_never_leaks_value() {
        let secret = Secret::new("s.hvs.SUPER-SECRET-TOKEN".to_string());
        let json = serde_json::to_string(&secret).expect("serialize");
        assert_eq!(json, "\"[REDACTED]\"");
        assert!(!json.contains("SUPER-SECRET-TOKEN"));
    }

    #[test]
    fn deserialize_is_faithful_and_value_readable_via_expose() {
        let secret: Secret<String> = serde_json::from_str("\"real-value\"").expect("deserialize");
        assert_eq!(secret.expose(), "real-value");
    }

    #[test]
    fn debug_and_display_also_redact() {
        let secret = Secret::new("p4ssw0rd".to_string());
        assert_eq!(format!("{secret:?}"), "[REDACTED]");
        assert_eq!(format!("{secret}"), "[REDACTED]");
    }
}
