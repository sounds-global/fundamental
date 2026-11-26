//! [`ShownError`] — what a page showing an error may know about it.

use core::num::NonZeroU64;

use super::Error;

/// The facts of an error a page is showing a person: its code, status and retry delay.
///
/// It holds no message, so a page cannot echo what it was never given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShownError {
    code: &'static str,
    status: u16,
    retry_after_secs: Option<NonZeroU64>,
}

impl ShownError {
    /// The facts of `error`. A retry delay of zero is no advice, as it is for `Retry-After`.
    #[must_use]
    pub fn of(error: &dyn Error) -> Self {
        Self {
            code: error.error_code(),
            status: error.http_status_code(),
            retry_after_secs: error.retry_after_secs().and_then(NonZeroU64::new),
        }
    }

    /// The error's stable code, such as `A7D_SEC_001`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// The HTTP status the error answers with.
    #[must_use]
    pub fn status(&self) -> u16 {
        self.status
    }

    /// How many seconds the server asked the caller to wait, when it asked.
    #[must_use]
    pub fn retry_after_secs(&self) -> Option<NonZeroU64> {
        self.retry_after_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;
    use std::fmt;

    #[derive(Debug)]
    struct Throttled(u64);

    impl fmt::Display for Throttled {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("throttled")
        }
    }

    impl core::error::Error for Throttled {}

    impl Error for Throttled {
        fn error_code(&self) -> &'static str {
            "TEST_429"
        }

        fn display_message(&self) -> Cow<'_, str> {
            Cow::Borrowed("Try again in a moment.")
        }

        fn developer_message(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed("bucket 10.0.0.0/24 exhausted"))
        }

        fn http_status_code(&self) -> u16 {
            429
        }

        fn retry_after_secs(&self) -> Option<u64> {
            Some(self.0)
        }
    }

    #[test]
    fn it_keeps_the_code_the_status_and_the_delay() {
        let shown = ShownError::of(&Throttled(30));
        assert_eq!(shown.code(), "TEST_429");
        assert_eq!(shown.status(), 429);
        assert_eq!(shown.retry_after_secs().map(NonZeroU64::get), Some(30));
    }

    #[test]
    fn a_zero_delay_is_no_advice() {
        assert_eq!(ShownError::of(&Throttled(0)).retry_after_secs(), None);
    }

    #[test]
    fn nothing_of_either_message_reaches_it() {
        let shown = format!("{:?}", ShownError::of(&Throttled(30)));
        assert!(!shown.contains("bucket"), "{shown}");
        assert!(!shown.contains("moment"), "{shown}");
    }
}
