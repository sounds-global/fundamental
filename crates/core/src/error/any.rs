//! Type-erased carrier for any Fundamental [`Error`].

#[cfg(not(feature = "std"))]
use alloc::sync::Arc;

#[cfg(not(feature = "std"))]
use alloc::borrow::Cow;
#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::borrow::Cow;
#[cfg(feature = "std")]
use std::string::String;
#[cfg(feature = "std")]
use std::sync::Arc;

use super::{Error, ErrorOverride};
use crate::validate::ValidationError;

/// A type-erased Fundamental [`Error`] that delegates everything to the wrapped error.
///
/// It must never implement [`Error`]: the blanket `From<E: Error>` impl relies on that for coherence.
#[derive(Clone)]
pub struct AnyError(Arc<dyn Error>);

impl AnyError {
    /// Wraps an error, erasing its concrete type.
    pub fn new(error: impl Error) -> Self {
        Self(Arc::new(error))
    }

    /// Returns the wrapped error.
    pub fn get(&self) -> &dyn Error {
        self.0.as_ref()
    }

    /// Attempts to recover the wrapped error's concrete type.
    pub fn downcast_ref<E: Error>(&self) -> Option<&E> {
        let inner: &(dyn core::error::Error + 'static) = self.0.as_ref();
        inner.downcast_ref::<E>()
    }
}

/// Any Fundamental [`Error`] erases into an `AnyError`; coherent only because `AnyError` is not an [`Error`].
impl<E: Error> From<E> for AnyError {
    fn from(error: E) -> Self {
        Self::new(error)
    }
}

impl core::fmt::Debug for AnyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(&self.0, f)
    }
}

impl core::fmt::Display for AnyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

impl core::error::Error for AnyError {
    /// Delegates to the wrapped error's source: the carrier is the wrapped error in a chain.
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        let inner: &(dyn core::error::Error + 'static) = self.0.as_ref();
        inner.source()
    }
}

// Crosses Leptos's hydration boundary as its user-facing message only; the concrete error is erased.
impl serde::Serialize for AnyError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.0.display_message().as_ref())
    }
}

impl<'de> serde::Deserialize<'de> for AnyError {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let message = <String as serde::Deserialize>::deserialize(deserializer)?;
        Ok(Self::new(HydratedError(message)))
    }
}

#[derive(Debug)]
struct HydratedError(String);

impl core::fmt::Display for HydratedError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl core::error::Error for HydratedError {}

impl Error for HydratedError {
    fn error_code(&self) -> &'static str {
        "REMOTE_000"
    }

    fn display_message(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.0)
    }
}

/// Full delegation: an `#[overrides]` `AnyError` field makes the variant transparent.
impl ErrorOverride for AnyError {
    fn override_error_code(&self) -> Option<&'static str> {
        Some(self.0.error_code())
    }

    fn override_display_message(&self) -> Option<Cow<'_, str>> {
        Some(self.0.display_message())
    }

    fn override_developer_message(&self) -> Option<Cow<'_, str>> {
        self.0.developer_message()
    }

    fn override_http_status_code(&self) -> Option<u16> {
        Some(self.0.http_status_code())
    }

    fn override_is_retryable(&self) -> Option<bool> {
        Some(self.0.is_retryable())
    }

    /// Delegated so a precise retry delay survives the carrier.
    fn override_retry_after_secs(&self) -> Option<Option<u64>> {
        Some(self.0.retry_after_secs())
    }

    fn override_message_numbers(&self) -> Option<Vec<(&'static str, i64)>> {
        Some(self.0.message_numbers())
    }

    fn override_field_errors(&self) -> Option<Vec<ValidationError>> {
        Some(self.0.field_errors())
    }

    fn override_exit_code(&self) -> Option<i32> {
        Some(self.0.exit_code())
    }
}

#[cfg(test)]
mod tests {
    use super::AnyError;
    use crate::Error;
    use std::borrow::Cow;

    #[derive(Debug)]
    struct SampleError;

    impl core::fmt::Display for SampleError {
        fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            formatter.write_str("sample error")
        }
    }

    impl core::error::Error for SampleError {}

    impl Error for SampleError {
        fn error_code(&self) -> &'static str {
            "TEST_001"
        }

        fn display_message(&self) -> Cow<'_, str> {
            Cow::Borrowed("sample error")
        }
    }

    #[test]
    fn cloning_keeps_the_erased_error_and_its_type() {
        fn assert_clone<T: Clone>() {}
        assert_clone::<Result<String, AnyError>>();

        let error = AnyError::new(SampleError);
        let cloned = error.clone();

        assert_eq!(cloned.to_string(), "sample error");
        assert!(cloned.downcast_ref::<SampleError>().is_some());
    }

    #[test]
    fn passes_on_the_refused_fields_of_the_error_it_carries() {
        use super::ErrorOverride;
        use crate::validate::ValidationErrors;

        let mut refused = ValidationErrors::new();
        refused.add_for_field("email", "taken", "Someone already has this address.");
        let carried = AnyError::new(refused.clone());

        assert_eq!(
            carried.override_field_errors(),
            Some(refused.errors().to_vec())
        );
        assert_eq!(carried.get().field_errors(), refused.errors().to_vec());
    }

    #[test]
    fn serialization_preserves_only_the_user_safe_message() {
        let error = AnyError::new(SampleError);
        let encoded = serde_json::to_string(&error).expect("AnyError serializes");
        let decoded: AnyError = serde_json::from_str(&encoded).expect("AnyError deserializes");

        assert_eq!(decoded.get().error_code(), "REMOTE_000");
        assert_eq!(decoded.get().display_message(), "sample error");
    }
}
