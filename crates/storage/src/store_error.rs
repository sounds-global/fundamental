//! Why the object store could not do what was asked.

use fundamental_core::{AnyError, ProviderName};
use fundamental_macros::Error;

/// A store refused, failed, or was handed something it will not keep; never holds content, keys or URLs.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The store's settings cannot make a store; every problem is listed.
    #[error(
        code = "BLOB_001",
        message = "The object storage settings are not valid",
        developer_message = "object storage settings: {problems}",
        status = 500
    )]
    InvalidSettings {
        /// Every problem found, joined.
        problems: String,
    },

    /// The provider could not be reached, or answered that it is busy.
    #[error(
        code = "BLOB_002",
        message = "File storage is briefly unavailable. Try again shortly",
        developer_message = "{provider} could not be reached or is busy",
        status = 503,
        retryable
    )]
    Unreachable {
        /// The provider.
        provider: ProviderName,
    },

    /// No object is stored under that key.
    #[error(code = "BLOB_003", message = "This file does not exist", status = 404)]
    NotFound,

    /// The provider refused the request with a status that will not change on retry.
    #[error(
        code = "BLOB_004",
        message = "File storage refused the request",
        developer_message = "{provider} answered {status}",
        status = 502
    )]
    Refused {
        /// The provider.
        provider: ProviderName,
        /// The provider's status.
        status: u16,
    },

    /// The provider answered something this adapter cannot read.
    #[error(
        code = "BLOB_005",
        message = "File storage answered unexpectedly",
        developer_message = "{provider} answered unreadably: {detail}",
        status = 502
    )]
    BadReply {
        /// The provider.
        provider: ProviderName,
        /// What was wrong with the reply — never its body.
        detail: &'static str,
    },

    /// The content being stored failed before it ended; nothing was kept.
    #[error(
        code = "BLOB_006",
        message = "The file did not arrive completely. Try again",
        status = 400,
        retryable
    )]
    BodyFailed {
        /// Why the content stopped.
        #[source]
        cause: AnyError,
    },

    /// The content ran past the bound it was stored under. Nothing was kept.
    #[error(
        code = "BLOB_007",
        message = "This file is larger than {max} bytes",
        status = 413
    )]
    TooLarge {
        /// The bound, in bytes.
        max: u64,
    },

    /// A private object did not open: altered, cut short, extended, moved, or read under the wrong key.
    #[error(
        code = "BLOB_008",
        message = "This file could not be read",
        developer_message = "a sealed object was refused: {reason}",
        status = 500
    )]
    Tampered {
        /// Why, for the operator.
        reason: &'static str,
    },

    /// A private object could not be sealed.
    #[error(
        code = "BLOB_009",
        message = "This file could not be stored",
        status = 500,
        retryable
    )]
    Sealing {
        /// What the envelope refused.
        #[source]
        cause: AnyError,
    },
}

#[cfg(test)]
mod tests {
    use super::StoreError;
    use fundamental_core::{AnyError, Error as _, ProviderName};

    #[test]
    fn every_variant_carries_its_own_code() {
        let provider = ProviderName::new("memory");
        let cause = || AnyError::new(StoreError::NotFound);
        let cases = [
            (
                StoreError::InvalidSettings {
                    problems: String::new(),
                },
                "BLOB_001",
                500,
                false,
            ),
            (
                StoreError::Unreachable {
                    provider: provider.clone(),
                },
                "BLOB_002",
                503,
                true,
            ),
            (StoreError::NotFound, "BLOB_003", 404, false),
            (
                StoreError::Refused {
                    provider: provider.clone(),
                    status: 403,
                },
                "BLOB_004",
                502,
                false,
            ),
            (
                StoreError::BadReply {
                    provider,
                    detail: "x",
                },
                "BLOB_005",
                502,
                false,
            ),
            (
                StoreError::BodyFailed { cause: cause() },
                "BLOB_006",
                400,
                true,
            ),
            (StoreError::TooLarge { max: 1 }, "BLOB_007", 413, false),
            (StoreError::Tampered { reason: "x" }, "BLOB_008", 500, false),
            (
                StoreError::Sealing { cause: cause() },
                "BLOB_009",
                500,
                true,
            ),
        ];
        for (error, code, status, retryable) in cases {
            assert_eq!(error.error_code(), code);
            assert_eq!(error.http_status_code(), status, "{code}");
            assert_eq!(error.is_retryable(), retryable, "{code}");
        }
    }
}
