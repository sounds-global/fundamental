//! [`ReturnPathError`] — why a return path was refused.

#[cfg(not(feature = "std"))]
use alloc::borrow::Cow;
#[cfg(not(feature = "std"))]
use alloc::string::ToString;
use core::fmt;
#[cfg(feature = "std")]
use std::borrow::Cow;

use crate::i18n::{ArgValue, translate};

/// Why a return path was refused.
///
/// Each shape would turn a sign-in redirect into an open redirect; all are `400`s, none
/// retryable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReturnPathError {
    /// Not a path at all — an absolute URL, a scheme, or a bare word.
    MustBeAbsolutePath,

    /// A protocol-relative URL, or one wearing a backslash.
    ProtocolRelative,

    /// A control character, whitespace, or a backslash anywhere in the value.
    NotAPath,

    /// Longer than any route an application serves.
    TooLong {
        /// The bound that was exceeded.
        limit: usize,
    },

    /// A fragment.
    Fragment,

    /// A `?name=` value whose percent-encoding is malformed or does not decode to UTF-8.
    MalformedEncoding,
}

impl ReturnPathError {
    /// A stable code for logs and dashboards, unchanged by wording.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::MustBeAbsolutePath => "RTP_001",
            Self::ProtocolRelative => "RTP_002",
            Self::NotAPath => "RTP_003",
            Self::TooLong { .. } => "RTP_004",
            Self::Fragment => "RTP_005",
            Self::MalformedEncoding => "RTP_006",
        }
    }

    const fn key(&self) -> &'static str {
        match self {
            Self::MustBeAbsolutePath => "error.rtp_001",
            Self::ProtocolRelative => "error.rtp_002",
            Self::NotAPath => "error.rtp_003",
            Self::TooLong { .. } => "error.rtp_004",
            Self::Fragment => "error.rtp_005",
            Self::MalformedEncoding => "error.rtp_006",
        }
    }

    const fn source(&self) -> &'static str {
        match self {
            Self::MustBeAbsolutePath => "a return path must begin with a single '/'",
            Self::ProtocolRelative => "a return path may not begin with '//' or '/\\'",
            Self::NotAPath => {
                "a return path may not contain whitespace, a control character or a backslash"
            }
            Self::TooLong { .. } => "a return path may not exceed {limit} bytes",
            Self::Fragment => "a return path may not carry a fragment",
            Self::MalformedEncoding => "a return path's encoding is malformed",
        }
    }
}

impl fmt::Display for ReturnPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong { limit } => {
                write!(f, "a return path may not exceed {limit} bytes")
            }
            other => f.write_str(other.source()),
        }
    }
}

impl core::error::Error for ReturnPathError {}

impl crate::Error for ReturnPathError {
    fn error_code(&self) -> &'static str {
        self.code()
    }

    fn display_message(&self) -> Cow<'_, str> {
        match self {
            Self::TooLong { limit } => {
                let limit = i64::try_from(*limit).unwrap_or(i64::MAX);
                translate(self.key(), &[("limit", ArgValue::Int(limit))], || {
                    Cow::Owned(self.to_string())
                })
            }
            other => translate(other.key(), &[], || Cow::Borrowed(other.source())),
        }
    }

    fn http_status_code(&self) -> u16 {
        400
    }
}

#[cfg(test)]
mod tests {
    use super::ReturnPathError;
    use crate::Error as _;

    #[test]
    fn every_refusal_is_a_400_with_its_own_code() {
        let cases = [
            (ReturnPathError::MustBeAbsolutePath, "RTP_001"),
            (ReturnPathError::ProtocolRelative, "RTP_002"),
            (ReturnPathError::NotAPath, "RTP_003"),
            (ReturnPathError::TooLong { limit: 512 }, "RTP_004"),
            (ReturnPathError::Fragment, "RTP_005"),
            (ReturnPathError::MalformedEncoding, "RTP_006"),
        ];
        for (error, code) in cases {
            assert_eq!(error.error_code(), code);
            assert_eq!(error.http_status_code(), 400);
            assert!(!error.is_retryable(), "{code} is not retryable");
        }
    }

    #[test]
    fn every_message_is_keyed_by_its_code() {
        let every = [
            ReturnPathError::MustBeAbsolutePath,
            ReturnPathError::ProtocolRelative,
            ReturnPathError::NotAPath,
            ReturnPathError::TooLong { limit: 512 },
            ReturnPathError::Fragment,
            ReturnPathError::MalformedEncoding,
        ];
        for error in every {
            assert_eq!(
                error.key(),
                format!("error.{}", error.code().to_lowercase()),
                "the key the derive would have chosen"
            );
        }
    }

    #[test]
    fn the_bound_is_named_in_the_message() {
        assert_eq!(
            ReturnPathError::TooLong { limit: 512 }.display_message(),
            "a return path may not exceed 512 bytes"
        );
    }
}
