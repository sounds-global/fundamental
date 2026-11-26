//! Field-driven overrides for derived [`Error`](crate::error::Error) metadata.

#[cfg(not(feature = "std"))]
use alloc::borrow::Cow;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::borrow::Cow;

use super::Error;
use crate::validate::ValidationError;

/// Per-field override hook for derived [`Error`] metadata.
///
/// # Example
///
/// ```no_run
/// use fundamental_core::error::ErrorOverride;
///
/// enum ConstraintKind { Unique, ForeignKey }
///
/// impl ErrorOverride for ConstraintKind {
///     fn override_http_status_code(&self) -> Option<u16> {
///         match self {
///             Self::Unique => Some(409),
///             _ => None,
///         }
///     }
/// }
/// ```
pub trait ErrorOverride {
    /// Override the machine-readable error code, or `None` to keep the declared one.
    fn override_error_code(&self) -> Option<&'static str> {
        None
    }

    /// Override the user-safe display message, or `None` to keep the declared one.
    fn override_display_message(&self) -> Option<Cow<'_, str>> {
        None
    }

    /// Override the developer message, or `None` to keep the declared one.
    fn override_developer_message(&self) -> Option<Cow<'_, str>> {
        None
    }

    /// Override the HTTP status code, or `None` to keep the declared one.
    fn override_http_status_code(&self) -> Option<u16> {
        None
    }

    /// Override the retryable flag, or `None` to keep the declared one.
    fn override_is_retryable(&self) -> Option<bool> {
        None
    }

    /// Override the retry delay, or `None` to keep the declared one.
    ///
    /// The outer `Option` says whether this source has an opinion; `Some(None)` means no delay advice.
    fn override_retry_after_secs(&self) -> Option<Option<u64>> {
        None
    }

    /// Override the message's whole-number arguments, or `None` to keep the declared ones.
    fn override_message_numbers(&self) -> Option<Vec<(&'static str, i64)>> {
        None
    }

    /// Override the refused fields, or `None` to keep the declared ones.
    fn override_field_errors(&self) -> Option<Vec<ValidationError>> {
        None
    }

    /// Override the process exit code, or `None` to keep the default.
    fn override_exit_code(&self) -> Option<i32> {
        None
    }
}

/// Every Fundamental [`Error`] is a full-delegation override source.
impl<T: Error> ErrorOverride for T {
    fn override_error_code(&self) -> Option<&'static str> {
        Some(Error::error_code(self))
    }

    fn override_display_message(&self) -> Option<Cow<'_, str>> {
        Some(Error::display_message(self))
    }

    fn override_developer_message(&self) -> Option<Cow<'_, str>> {
        Error::developer_message(self)
    }

    fn override_http_status_code(&self) -> Option<u16> {
        Some(Error::http_status_code(self))
    }

    fn override_is_retryable(&self) -> Option<bool> {
        Some(Error::is_retryable(self))
    }

    fn override_retry_after_secs(&self) -> Option<Option<u64>> {
        Some(Error::retry_after_secs(self))
    }

    fn override_message_numbers(&self) -> Option<Vec<(&'static str, i64)>> {
        Some(Error::message_numbers(self))
    }

    fn override_field_errors(&self) -> Option<Vec<ValidationError>> {
        Some(Error::field_errors(self))
    }

    fn override_exit_code(&self) -> Option<i32> {
        Some(Error::exit_code(self))
    }
}

/// `Option<T>` lifts any override source; `None` never overrides.
impl<T: ErrorOverride> ErrorOverride for Option<T> {
    fn override_error_code(&self) -> Option<&'static str> {
        self.as_ref().and_then(ErrorOverride::override_error_code)
    }

    fn override_display_message(&self) -> Option<Cow<'_, str>> {
        self.as_ref()
            .and_then(ErrorOverride::override_display_message)
    }

    fn override_developer_message(&self) -> Option<Cow<'_, str>> {
        self.as_ref()
            .and_then(ErrorOverride::override_developer_message)
    }

    fn override_http_status_code(&self) -> Option<u16> {
        self.as_ref()
            .and_then(ErrorOverride::override_http_status_code)
    }

    fn override_is_retryable(&self) -> Option<bool> {
        self.as_ref().and_then(ErrorOverride::override_is_retryable)
    }

    fn override_message_numbers(&self) -> Option<Vec<(&'static str, i64)>> {
        self.as_ref()
            .and_then(ErrorOverride::override_message_numbers)
    }

    fn override_field_errors(&self) -> Option<Vec<ValidationError>> {
        self.as_ref().and_then(ErrorOverride::override_field_errors)
    }

    fn override_exit_code(&self) -> Option<i32> {
        self.as_ref().and_then(ErrorOverride::override_exit_code)
    }
}
