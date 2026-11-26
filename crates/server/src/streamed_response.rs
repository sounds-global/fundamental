use std::fmt;
use std::pin::Pin;

use bytes::Bytes;
use fundamental_core::AnyError;
use futures_core::Stream;

/// Internal response metadata consumed by the API backend, never sent to the caller.
#[doc(hidden)]
pub const INTERNAL_CSP_NONCE_HEADER: &str = "x-fundamental-internal-csp-nonce";

/// What a [streaming route](crate::Route::streaming_response) answers: a content type, an optional exact length, and the bytes.
pub struct StreamedResponse {
    content_type: String,
    len: Option<u64>,
    stream: Pin<Box<dyn Stream<Item = Result<Bytes, AnyError>> + Send>>,
}

impl StreamedResponse {
    /// A body of exactly `len` bytes of `content_type`.
    pub fn new(
        content_type: impl Into<String>,
        len: u64,
        stream: impl Stream<Item = Result<Bytes, AnyError>> + Send + 'static,
    ) -> Self {
        Self {
            content_type: content_type.into(),
            len: Some(len),
            stream: Box::pin(stream),
        }
    }

    /// A streamed body whose final length is not known before its first byte is written.
    pub fn with_unknown_length(
        content_type: impl Into<String>,
        stream: impl Stream<Item = Result<Bytes, AnyError>> + Send + 'static,
    ) -> Self {
        Self {
            content_type: content_type.into(),
            len: None,
            stream: Box::pin(stream),
        }
    }

    /// The media type.
    #[must_use]
    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    /// The exact length, or `0` for an unknown length; see [`exact_len`](Self::exact_len).
    #[must_use]
    pub fn len(&self) -> u64 {
        self.len.unwrap_or(0)
    }

    /// The exact length, when the stream knows it before writing begins.
    #[must_use]
    pub fn exact_len(&self) -> Option<u64> {
        self.len
    }

    /// Whether the body is known to be empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == Some(0)
    }

    /// The bytes, for the backend that writes them.
    #[must_use]
    pub fn into_stream(self) -> Pin<Box<dyn Stream<Item = Result<Bytes, AnyError>> + Send>> {
        self.stream
    }
}

impl fmt::Debug for StreamedResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamedResponse")
            .field("content_type", &self.content_type)
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}
