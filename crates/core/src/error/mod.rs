//! Error trait for consistent error handling.

#[cfg(not(feature = "std"))]
use alloc::borrow::Cow;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::borrow::Cow;

mod any;
mod categories;
mod context;
mod opt_display;
mod overridable;
#[cfg(feature = "json")]
mod payload_rejection;
#[cfg(feature = "std")]
mod refusal;
mod shown_error;

pub use any::AnyError;
pub use categories::*;
pub use context::WithContext;
pub use opt_display::OptDisplay;
pub use overridable::ErrorOverride;
#[cfg(feature = "json")]
pub use payload_rejection::PayloadRejection;
#[cfg(feature = "std")]
pub use refusal::Refusal;
pub use shown_error::ShownError;

use crate::validate::ValidationError;

/// The foundational error trait for all Fundamental errors.
pub trait Error: core::error::Error + Send + Sync + 'static {
    /// Returns a machine-readable error code.
    ///
    /// Stable, in the pattern `CATEGORY_NNN`.
    fn error_code(&self) -> &'static str;

    /// Returns a user-safe error message.
    ///
    /// It must not contain sensitive information.
    fn display_message(&self) -> Cow<'_, str>;

    /// Returns a detailed message for developers.
    ///
    /// Only shown in development or logged server-side.
    fn developer_message(&self) -> Option<Cow<'_, str>> {
        None
    }

    /// Returns the HTTP status code for this error.
    fn http_status_code(&self) -> u16 {
        500
    }

    /// Returns whether this error is retryable.
    fn is_retryable(&self) -> bool {
        false
    }

    /// Returns how long the caller should wait before retrying, in whole seconds.
    ///
    /// Never `Some(0)`: a client reads it as "retry now".
    fn retry_after_secs(&self) -> Option<u64> {
        None
    }

    /// Returns the whole-number arguments of the user-safe message, by name.
    ///
    /// Only numbers may leave the request that raised the error: a string argument may be personal.
    fn message_numbers(&self) -> Vec<(&'static str, i64)> {
        Vec::new()
    }

    /// Returns the refusal of each field this error names, with its user-safe message.
    fn field_errors(&self) -> Vec<ValidationError> {
        Vec::new()
    }

    /// Returns the process exit code for CLI / process surfaces.
    fn exit_code(&self) -> i32 {
        1
    }

    /// Constructs an "unauthenticated" error instance.
    ///
    /// Used by `#[action]` to enforce `auth = "required"`; `#[derive(Error)]` generates it from the `status = 401` variant.
    fn unauthenticated() -> Self
    where
        Self: Sized,
    {
        panic!(
            "Error::unauthenticated() not implemented. \
             Add a variant with `status = 401` to your error enum, \
             or use `auth = \"none\"` on the action."
        )
    }
}
