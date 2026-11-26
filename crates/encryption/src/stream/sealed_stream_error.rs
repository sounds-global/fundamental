use crate::{KeyringError, StreamRefusal};
use fundamental_macros::Error;

/// A [`SegmentSealer`](crate::SegmentSealer) could not seal, or a
/// [`SegmentOpener`](crate::SegmentOpener) refused.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SealedStreamError {
    /// The operating system could not supply a key or a nonce prefix. Nothing was sealed.
    #[error(
        code = "STREAM_001",
        message = "Could not gather entropy to seal a stream",
        status = 500,
        retryable
    )]
    Entropy,

    /// A header or segment was refused, alike whatever the reason; `reason` is for the operator.
    #[error(
        code = "STREAM_002",
        message = "A sealed stream was refused",
        developer_message = "A sealed stream was refused: {reason}",
        status = 500
    )]
    Refused {
        /// Why — never returned to whoever supplied the stream.
        reason: StreamRefusal,
    },

    /// The content needs more segments than the STREAM counter can number.
    #[error(
        code = "STREAM_003",
        message = "The content is too long to seal as one stream",
        status = 413
    )]
    TooLong,

    /// The cipher refused to seal a segment.
    #[error(
        code = "STREAM_004",
        message = "Could not seal a stream segment",
        status = 500
    )]
    Encryption,

    /// The keyring that wraps content keys refused.
    #[error(transparent)]
    Keyring(#[from] KeyringError),
}

#[cfg(test)]
mod tests {
    use super::SealedStreamError;
    use crate::StreamRefusal;
    use fundamental_core::Error as _;

    #[test]
    fn every_variant_carries_its_own_code_and_status() {
        let cases = [
            (SealedStreamError::Entropy, "STREAM_001", 500),
            (
                SealedStreamError::Refused {
                    reason: StreamRefusal::Length,
                },
                "STREAM_002",
                500,
            ),
            (SealedStreamError::TooLong, "STREAM_003", 413),
            (SealedStreamError::Encryption, "STREAM_004", 500),
        ];
        for (error, code, status) in cases {
            assert_eq!(error.error_code(), code);
            assert_eq!(error.http_status_code(), status);
        }
    }

    #[test]
    fn a_refusal_names_no_reason_to_the_sender() {
        let error = SealedStreamError::Refused {
            reason: StreamRefusal::Unauthenticated,
        };
        assert!(!error.to_string().contains("unauthenticated"));
        assert!(
            error
                .developer_message()
                .is_some_and(|message| message.contains("unauthenticated"))
        );
    }
}
