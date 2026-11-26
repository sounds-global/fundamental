#[cfg(not(feature = "std"))]
use alloc::string::String;

/// Errors that can accumulate context as they propagate up the call stack.
pub trait WithContext: Sized {
    /// Adds a context breadcrumb to this error.
    fn with_context(self, ctx: impl Into<String>) -> Self;

    /// Returns the accumulated context breadcrumbs (oldest first).
    fn context_chain(&self) -> &[String];
}
