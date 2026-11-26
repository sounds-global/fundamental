//! Ephemeral, never-persisted secret wrapper.

use core::fmt;

use crate::secrets::Secret;

/// A sensitive value that lives only in memory and is never persisted.
///
/// The `Model` derive never persists it; for a secret stored encrypted at rest use [`Secret<T>`].
///
/// # Example
///
/// ```
/// use fundamental_core::secrets::InMemorySecret;
///
/// let mut token = InMemorySecret::new("s.hvs.TOKEN".to_string());
///
/// // Safe to log — shows [REDACTED]
/// println!("token: {token:?}");
///
/// // Truthful access — present yields Some, absent yields None
/// assert_eq!(token.expose().map(String::as_str), Some("s.hvs.TOKEN"));
/// let _taken = token.take();
/// assert_eq!(token.expose(), None);
/// ```
pub struct InMemorySecret<T>(Option<T>);

impl<T> InMemorySecret<T> {
    /// Creates a present in-memory secret holding `value`.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self(Some(value))
    }

    /// Creates an absent in-memory secret (no value yet).
    ///
    /// The state of a skipped model field after a row is read back.
    #[must_use]
    pub fn absent() -> Self {
        Self(None)
    }

    /// Whether a value is currently present.
    #[must_use]
    pub fn is_present(&self) -> bool {
        self.0.is_some()
    }

    /// Whether the secret is currently absent.
    #[must_use]
    pub fn is_absent(&self) -> bool {
        self.0.is_none()
    }

    /// Exposes the value by reference, or `None` when absent.
    #[must_use]
    pub fn expose(&self) -> Option<&T> {
        self.0.as_ref()
    }

    /// Exposes the value by mutable reference, or `None` when absent.
    pub fn expose_mut(&mut self) -> Option<&mut T> {
        self.0.as_mut()
    }

    /// Consumes the secret and returns the inner value, or `None` when absent.
    #[must_use]
    pub fn expose_owned(self) -> Option<T> {
        self.0
    }

    /// Sets (or replaces) the value, returning the previous one if present.
    pub fn set(&mut self, value: T) -> Option<T> {
        self.0.replace(value)
    }

    /// Takes the value out, leaving the secret absent.
    pub fn take(&mut self) -> Option<T> {
        self.0.take()
    }

    /// Maps the inner value to a new type without exposing it; absent stays absent.
    pub fn map<U, F>(self, f: F) -> InMemorySecret<U>
    where
        F: FnOnce(T) -> U,
    {
        InMemorySecret(self.0.map(f))
    }
}

impl<T: Clone> Clone for InMemorySecret<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> fmt::Debug for InMemorySecret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_some() {
            "[REDACTED]"
        } else {
            "[UNSET]"
        })
    }
}

impl<T> fmt::Display for InMemorySecret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_some() {
            "[REDACTED]"
        } else {
            "[UNSET]"
        })
    }
}

// Absent by default, with no `T: Default` bound.
impl<T> Default for InMemorySecret<T> {
    fn default() -> Self {
        Self(None)
    }
}

// No `PartialEq`, for the reason `Secret<T>` has none (see `secret.rs`).

impl<T> From<Secret<T>> for InMemorySecret<T> {
    fn from(secret: Secret<T>) -> Self {
        Self(Some(secret.expose_owned()))
    }
}

// Serialization redacts (an absent secret is null); deserialization is faithful.
impl<T> serde::Serialize for InMemorySecret<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match &self.0 {
            Some(_) => serializer.serialize_str("[REDACTED]"),
            None => serializer.serialize_none(),
        }
    }
}

impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for InMemorySecret<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Option::<T>::deserialize(deserializer).map(InMemorySecret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_serialize_redacts_and_never_leaks_value() {
        let secret = InMemorySecret::new("s.hvs.SUPER-SECRET-TOKEN".to_string());
        let json = serde_json::to_string(&secret).expect("serialize");
        assert_eq!(json, "\"[REDACTED]\"");
        assert!(!json.contains("SUPER-SECRET-TOKEN"));
    }

    #[test]
    fn absent_serializes_as_null() {
        let secret: InMemorySecret<String> = InMemorySecret::absent();
        let json = serde_json::to_string(&secret).expect("serialize");
        assert_eq!(json, "null");
    }

    #[test]
    fn deserialize_is_faithful_for_present_and_absent() {
        let present: InMemorySecret<String> =
            serde_json::from_str("\"real-value\"").expect("deserialize");
        assert_eq!(present.expose().map(String::as_str), Some("real-value"));

        let absent: InMemorySecret<String> = serde_json::from_str("null").expect("deserialize");
        assert!(absent.is_absent());
        assert_eq!(absent.expose(), None);
    }

    #[test]
    fn debug_and_display_distinguish_present_from_absent_without_leaking() {
        let present = InMemorySecret::new("p4ssw0rd".to_string());
        assert_eq!(format!("{present:?}"), "[REDACTED]");
        assert_eq!(format!("{present}"), "[REDACTED]");

        let absent: InMemorySecret<String> = InMemorySecret::absent();
        assert_eq!(format!("{absent:?}"), "[UNSET]");
        assert_eq!(format!("{absent}"), "[UNSET]");
    }

    #[test]
    fn absent_secret_never_yields_a_fabricated_value() {
        let secret: InMemorySecret<String> = InMemorySecret::absent();
        assert!(secret.is_absent());
        assert_eq!(secret.expose(), None);
        assert_eq!(secret.expose_owned(), None);
    }

    #[test]
    fn set_populates_and_returns_previous() {
        let mut secret: InMemorySecret<String> = InMemorySecret::absent();
        assert_eq!(secret.set("first".to_string()), None);
        assert_eq!(secret.set("second".to_string()), Some("first".to_string()));
        assert_eq!(secret.expose().map(String::as_str), Some("second"));
    }

    #[test]
    fn take_leaves_secret_absent() {
        let mut secret = InMemorySecret::new("v".to_string());
        assert_eq!(secret.take(), Some("v".to_string()));
        assert!(secret.is_absent());
        assert_eq!(secret.take(), None);
    }

    #[test]
    fn expose_mut_modifies_present_value() {
        let mut secret = InMemorySecret::new("base".to_string());
        secret.expose_mut().expect("present").push_str("-extra");
        assert_eq!(secret.expose().map(String::as_str), Some("base-extra"));

        let mut absent: InMemorySecret<String> = InMemorySecret::absent();
        assert!(absent.expose_mut().is_none());
    }

    #[test]
    fn map_transforms_present_and_preserves_absent() {
        let present = InMemorySecret::new(21i32).map(|n| n * 2);
        assert_eq!(present.expose(), Some(&42));

        let absent = InMemorySecret::<i32>::absent().map(|n| n * 2);
        assert!(absent.is_absent());
    }

    #[test]
    fn default_is_absent_without_t_default_bound() {
        // `NoDefault` has no `Default` impl, yet `InMemorySecret<NoDefault>` does.
        struct NoDefault;
        let secret: InMemorySecret<NoDefault> = InMemorySecret::default();
        assert!(secret.is_absent());
    }

    #[test]
    fn a_clone_carries_the_same_inner_state() {
        let a = InMemorySecret::new("same".to_string());
        let b = InMemorySecret::new("same".to_string());
        let c = InMemorySecret::new("different".to_string());
        let absent: InMemorySecret<String> = InMemorySecret::absent();
        assert_eq!(a.expose(), b.expose());
        assert_eq!(a.clone().expose(), a.expose());
        assert_ne!(a.expose(), c.expose());
        assert!(absent.is_absent());
        assert!(!a.is_absent());
    }

    #[test]
    fn converts_from_secret() {
        let persisted = Secret::new("carried".to_string());
        let ephemeral: InMemorySecret<String> = persisted.into();
        assert_eq!(ephemeral.expose().map(String::as_str), Some("carried"));
    }
}
