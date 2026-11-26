use std::borrow::Cow;

use fundamental_core::Error;
use fundamental_core::error::ShownError;

use crate::StreamedResponse;
use serde_json::{Value, json};

/// The content type a JSON response carries.
const JSON_CONTENT_TYPE: &str = "application/json";

/// The content type a server-rendered HTML response carries.
const HTML_CONTENT_TYPE: &str = "text/html; charset=utf-8";

/// A backend-neutral HTTP response: a status code, a body, and its content type.
#[derive(Debug)]
pub struct ServerResponse {
    status: u16,
    body: Vec<u8>,
    content_type: Cow<'static, str>,
    headers: Vec<(String, String)>,
    // Per response: on one surface a prefixed page is language-invariant while `/` is negotiated.
    language_invariant: bool,
    preference_invariant: bool,
    // When set, `body` is empty and `content_type` is the stream's.
    stream: Option<StreamedResponse>,
    // Set only by `from_error`, and cleared once a page is put in its place.
    shown: Option<ShownError>,
}

impl ServerResponse {
    /// Creates a JSON response with `status` and `body`.
    #[must_use]
    pub fn new(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            body,
            content_type: Cow::Borrowed(JSON_CONTENT_TYPE),
            headers: Vec::new(),
            language_invariant: false,
            preference_invariant: false,
            stream: None,
            shown: None,
        }
    }

    /// A response whose body is `stream`, under its own content type and length.
    #[must_use]
    pub fn streamed(status: u16, stream: StreamedResponse) -> Self {
        let mut response = Self::new(status, Vec::new());
        response.content_type = Cow::Owned(stream.content_type().to_owned());
        response.stream = Some(stream);
        response
    }

    /// Takes the streamed body, if the response has one.
    pub fn take_stream(&mut self) -> Option<StreamedResponse> {
        self.stream.take()
    }

    /// Whether the body is streamed.
    #[must_use]
    pub fn is_streamed(&self) -> bool {
        self.stream.is_some()
    }

    /// This response with a streamed body read into memory; a body that fails part-way answers that failure.
    pub async fn buffered(mut self) -> Self {
        let Some(stream) = self.stream.take() else {
            return self;
        };
        let mut stream = stream.into_stream();
        let mut body = Vec::new();
        while let Some(chunk) =
            std::future::poll_fn(|cx| futures_core::Stream::poll_next(stream.as_mut(), cx)).await
        {
            match chunk {
                Ok(chunk) => body.extend_from_slice(&chunk),
                Err(error) => return Self::from_error(error.get()),
            }
        }
        self.body = body;
        self
    }

    /// Creates an HTML response with `status` and `body`.
    #[must_use]
    pub fn html(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            body,
            content_type: Cow::Borrowed(HTML_CONTENT_TYPE),
            headers: Vec::new(),
            language_invariant: false,
            preference_invariant: false,
            stream: None,
            shown: None,
        }
    }

    /// Sets the response's content type, returning the updated response.
    #[must_use]
    pub fn with_content_type(mut self, content_type: impl Into<Cow<'static, str>>) -> Self {
        self.content_type = content_type.into();
        self
    }

    /// Sets the response status, returning the updated response.
    #[must_use]
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    /// Appends a response header `(name, value)`, returning the updated response.
    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Sets `(name, value)` as the header's only value, case-insensitively, returning the updated response.
    #[must_use]
    pub fn with_replaced_header(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        let name = name.into();
        self.headers
            .retain(|(recorded, _)| !recorded.eq_ignore_ascii_case(&name));
        self.headers.push((name, value.into()));
        self
    }

    /// The extra response headers a backend should stamp on the response.
    #[must_use]
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    /// Records that this response is the same whatever language the caller asked for, so `Vary` leaves the language out.
    #[must_use]
    pub fn with_language_invariant(mut self) -> Self {
        self.language_invariant = true;
        self
    }

    /// Records that this response is byte-identical whatever the caller pinned: a static asset, and nothing else.
    #[must_use]
    pub fn with_preference_invariant(mut self) -> Self {
        self.preference_invariant = true;
        self
    }

    /// Whether this response is byte-identical whatever the caller pinned.
    #[must_use]
    pub fn preference_invariant(&self) -> bool {
        self.preference_invariant
    }

    /// Whether this response is the same whatever language the caller asked for.
    #[must_use]
    pub fn language_invariant(&self) -> bool {
        self.language_invariant
    }

    /// Renders any framework [`Error`] into its JSON error envelope, with only the user-safe message.
    ///
    /// Adds `fields` for refused fields, and a `Retry-After` header when the error advises one.
    #[must_use]
    pub fn from_error(error: &dyn Error) -> Self {
        let status = error.http_status_code();
        let mut envelope = json!({
            "error": error.error_code(),
            "message": error.display_message(),
            "status": status,
        });
        let fields: Vec<_> = error
            .field_errors()
            .into_iter()
            .map(|refused| {
                json!({
                    "field": refused.field,
                    "code": refused.code,
                    "message": refused.message,
                })
            })
            .collect();
        if !fields.is_empty() {
            envelope["fields"] = Value::Array(fields);
        }
        // Serializing a `serde_json::Value` of owned strings cannot fail.
        let body = serde_json::to_vec(&envelope).unwrap_or_else(|_| b"{}".to_vec());
        let mut response = Self::new(status, body);
        response.shown = Some(ShownError::of(error));
        // RFC 9110 §10.2.3 delay-seconds; a zero would read as "retry immediately".
        match error.retry_after_secs() {
            Some(seconds) if seconds > 0 => {
                response.with_header("Retry-After", seconds.to_string())
            }
            _ => response,
        }
    }

    /// The error this response is the envelope of, while no page has been put in its place.
    #[must_use]
    pub fn shown_error(&self) -> Option<ShownError> {
        self.shown
    }

    /// This error envelope answered with `page`: the error's status and headers, the page's body, type and headers.
    #[must_use]
    pub(crate) fn answered_with(mut self, page: ServerResponse) -> Self {
        self.shown = None;
        self.body = page.body;
        self.content_type = page.content_type;
        self.stream = page.stream;
        self.headers.extend(page.headers);
        self
    }

    /// This error envelope, no longer offered to a page.
    #[must_use]
    pub(crate) fn unmarked(mut self) -> Self {
        self.shown = None;
        self
    }

    /// The HTTP status code.
    #[must_use]
    pub fn status(&self) -> u16 {
        self.status
    }

    /// The response body.
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// The content type a backend should stamp on the response.
    #[must_use]
    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    /// Consumes the response, returning its body.
    #[must_use]
    pub fn into_body(self) -> Vec<u8> {
        self.body
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ServerError;

    #[test]
    fn new_keeps_status_and_body_and_defaults_to_json() {
        let response = ServerResponse::new(200, b"{}".to_vec());
        assert_eq!(response.status(), 200);
        assert_eq!(response.body(), b"{}");
        assert_eq!(response.content_type(), "application/json");
    }

    #[test]
    fn html_sets_a_text_html_content_type() {
        let response = ServerResponse::html(200, b"<h1>hi</h1>".to_vec());
        assert_eq!(response.status(), 200);
        assert_eq!(response.body(), b"<h1>hi</h1>");
        assert_eq!(response.content_type(), "text/html; charset=utf-8");
    }

    #[test]
    fn with_content_type_overrides_the_default() {
        let response =
            ServerResponse::new(200, b"body".to_vec()).with_content_type("application/wasm");
        assert_eq!(response.content_type(), "application/wasm");
    }

    #[test]
    fn from_error_renders_the_stable_json_envelope() {
        let error = ServerError::NotFound {
            method: "GET".into(),
            path: crate::PathShape::new("/missing", |_| true),
        };
        let response = ServerResponse::from_error(&error);
        assert_eq!(response.status(), 404);
        assert_eq!(response.content_type(), "application/json");

        let value: serde_json::Value = serde_json::from_slice(response.body()).expect("json body");
        assert_eq!(value["error"], "API_002");
        assert_eq!(value["status"], 404);
        assert!(
            value["message"]
                .as_str()
                .expect("message string")
                .contains("/missing")
        );
    }

    #[test]
    fn from_error_lists_each_refused_field_with_its_user_safe_message() {
        let mut refused = fundamental_core::validate::ValidationErrors::new();
        refused.add(
            fundamental_core::validate::ValidationError::new(
                Some("email".to_owned()),
                "taken",
                "Someone already has this address.",
            )
            .with_developer_message("unique index users_email"),
        );
        let response = ServerResponse::from_error(&refused);
        assert_eq!(response.status(), 400);

        let value: serde_json::Value = serde_json::from_slice(response.body()).expect("json body");
        assert_eq!(
            value["fields"],
            json!([{"field": "email", "code": "taken", "message": "Someone already has this address."}])
        );
        assert!(!String::from_utf8_lossy(response.body()).contains("users_email"));
    }

    #[test]
    fn from_error_has_no_fields_when_the_error_refuses_none() {
        let error = ServerError::NotFound {
            method: "GET".into(),
            path: crate::PathShape::new("/missing", |_| true),
        };
        let value: serde_json::Value =
            serde_json::from_slice(ServerResponse::from_error(&error).body()).expect("json body");
        assert!(value.get("fields").is_none());
    }

    #[test]
    fn from_error_stamps_retry_after_only_when_the_error_advises_one() {
        #[derive(Debug)]
        struct Throttled(Option<u64>);

        impl core::fmt::Display for Throttled {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str("too many requests")
            }
        }
        impl core::error::Error for Throttled {}
        impl Error for Throttled {
            fn error_code(&self) -> &'static str {
                "THROTTLED"
            }
            fn display_message(&self) -> Cow<'_, str> {
                Cow::Borrowed("too many requests")
            }
            fn http_status_code(&self) -> u16 {
                429
            }
            fn is_retryable(&self) -> bool {
                true
            }
            fn retry_after_secs(&self) -> Option<u64> {
                self.0
            }
        }

        let advised = ServerResponse::from_error(&Throttled(Some(30)));
        assert_eq!(advised.status(), 429);
        assert_eq!(
            advised.headers(),
            &[("Retry-After".to_string(), "30".to_string())]
        );

        let silent = ServerResponse::from_error(&Throttled(None));
        assert!(
            silent.headers().is_empty(),
            "no advice must render no header, never `Retry-After: 0`"
        );
    }

    #[tokio::test]
    async fn a_streamed_body_is_buffered_whole_or_answered_as_its_failure() {
        use crate::{ServerError, StreamedResponse};
        use bytes::Bytes;
        use fundamental_core::AnyError;

        struct Pieces(Vec<Result<Bytes, AnyError>>);
        impl futures_core::Stream for Pieces {
            type Item = Result<Bytes, AnyError>;
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn poll_next(
                mut self: std::pin::Pin<&mut Self>,
                _: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Option<Self::Item>> {
                let next = (!self.0.is_empty()).then(|| self.0.remove(0));
                std::task::Poll::Ready(next)
            }
        }

        let whole = StreamedResponse::new(
            "text/plain",
            6,
            Pieces(vec![
                Ok(Bytes::from_static(b"abc")),
                Ok(Bytes::from_static(b"def")),
            ]),
        );
        let buffered = ServerResponse::streamed(200, whole).buffered().await;
        assert!(!buffered.is_streamed());
        assert_eq!(buffered.body(), b"abcdef");
        assert_eq!(buffered.content_type(), "text/plain");

        let broken = StreamedResponse::new(
            "text/plain",
            6,
            Pieces(vec![
                Ok(Bytes::from_static(b"abc")),
                Err(AnyError::new(ServerError::BodyInterrupted)),
            ]),
        );
        let answered = ServerResponse::streamed(200, broken).buffered().await;
        assert_eq!(
            answered.status(),
            400,
            "a truncated body is answered as its failure"
        );
    }
}
