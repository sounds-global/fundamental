use std::borrow::Cow;

use fundamental_core::ErrorOverride;
use fundamental_core::validate::ValidationError;
use fundamental_macros::Error;
use serde::Deserialize;

/// An error from invoking a web action from the browser.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum WebClientError {
    /// The call produced no usable HTTP response, or its input or output did not (de)serialize.
    #[error(
        code = "WEB_001",
        message = "The request could not be completed.",
        developer_message = "Web action transport failure: {0}",
        status = 502,
        retryable
    )]
    Transport(String),

    /// The server returned its error envelope; status and message are the server's own.
    #[error(
        code = "WEB_002",
        message = "The action could not be completed.",
        developer_message = "Remote web action failure",
        status = 500
    )]
    Remote {
        /// The server's reported status, message, and machine code.
        #[overrides]
        fault: RemoteFault,
    },
}

impl WebClientError {
    /// Builds the error for a non-success HTTP response from the server's `body`.
    #[must_use]
    pub fn from_error_response(status: u16, body: &str) -> Self {
        match serde_json::from_str::<ErrorEnvelope>(body) {
            Ok(envelope) => Self::Remote {
                fault: RemoteFault::new(envelope.error, envelope.message, status)
                    .with_fields(envelope.fields),
            },
            Err(_) => Self::Transport(format!("unexpected error response (HTTP {status})")),
        }
    }

    /// The server's machine-readable error code for a [`Remote`](Self::Remote) failure.
    #[must_use]
    pub fn remote_code(&self) -> Option<&str> {
        match self {
            Self::Remote { fault } => Some(fault.code()),
            _ => None,
        }
    }
}

/// The server's reported failure detail, refining its variant's status and message.
#[derive(Debug, Clone)]
pub struct RemoteFault {
    code: String,
    message: String,
    status: u16,
    fields: Vec<ValidationError>,
}

impl RemoteFault {
    /// Builds a fault from the envelope's machine `code`, user-safe `message`, and HTTP `status`.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>, status: u16) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            status,
            fields: Vec::new(),
        }
    }

    /// The same fault, refusing each of `fields` with its user-safe message.
    #[must_use]
    pub fn with_fields(mut self, fields: Vec<ValidationError>) -> Self {
        self.fields = fields;
        self
    }

    /// The fields the server refused, each with its user-safe message.
    #[must_use]
    pub fn fields(&self) -> &[ValidationError] {
        &self.fields
    }

    /// The server's machine-readable error code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The server's user-safe message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The server's HTTP status.
    #[must_use]
    pub fn status(&self) -> u16 {
        self.status
    }
}

impl ErrorOverride for RemoteFault {
    fn override_display_message(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(&self.message))
    }

    fn override_http_status_code(&self) -> Option<u16> {
        Some(self.status)
    }

    fn override_field_errors(&self) -> Option<Vec<ValidationError>> {
        Some(self.fields.clone())
    }

    fn override_developer_message(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Owned(format!(
            "remote web action failed: {} (HTTP {}): {}",
            self.code, self.status, self.message
        )))
    }
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: String,
    message: String,
    #[serde(default)]
    fields: Vec<ValidationError>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    #[test]
    fn transport_carries_the_framework_code_and_is_retryable() {
        let err = WebClientError::Transport("connection reset".into());
        assert_eq!(err.error_code(), "WEB_001");
        assert_eq!(err.http_status_code(), 502);
        assert!(err.is_retryable());
        assert_eq!(err.remote_code(), None);
    }

    #[test]
    fn remote_reflects_the_servers_status_and_message() {
        let body = r#"{"error":"DB_004","message":"Record not found","status":404}"#;
        let err = WebClientError::from_error_response(404, body);

        // The framework code stays; the remote application code survives as data.
        assert_eq!(err.error_code(), "WEB_002");
        assert_eq!(err.remote_code(), Some("DB_004"));
        assert_eq!(err.http_status_code(), 404);
        assert_eq!(err.display_message(), "Record not found");
    }

    #[test]
    fn remote_answers_each_refused_field_of_the_envelope() {
        let body = r#"{"error":"VALIDATION_ERROR","message":"Validation failed","status":400,
            "fields":[{"field":"email","code":"taken","message":"Someone already has this address."}]}"#;
        let err = WebClientError::from_error_response(400, body);

        assert_eq!(
            err.field_errors(),
            vec![ValidationError::new(
                Some("email".to_owned()),
                "taken",
                "Someone already has this address."
            )]
        );
    }

    #[test]
    fn remote_without_fields_refuses_none() {
        let body = r#"{"error":"DB_004","message":"Record not found","status":404}"#;
        assert!(
            WebClientError::from_error_response(404, body)
                .field_errors()
                .is_empty()
        );
    }

    #[test]
    fn a_body_that_is_not_an_envelope_is_a_transport_failure() {
        let err = WebClientError::from_error_response(500, "<html>oops</html>");
        assert_eq!(err.error_code(), "WEB_001");
        assert_eq!(err.http_status_code(), 502);
        assert!(err.developer_message().is_some_and(|m| m.contains("500")));
    }

    #[test]
    fn remote_status_comes_from_the_http_response_not_the_envelope() {
        // The envelope's own status field is ignored; the HTTP status is authoritative.
        let body = r#"{"error":"API_009","message":"Authentication required"}"#;
        let err = WebClientError::from_error_response(401, body);
        assert_eq!(err.http_status_code(), 401);
        assert_eq!(err.remote_code(), Some("API_009"));
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn the_servers_error_envelope_round_trips_to_the_client() {
        use fundamental_server::{ServerError, ServerResponse};

        let server_error = ServerError::unauthenticated();
        let response = ServerResponse::from_error(&server_error);
        let status = response.status();
        let body = std::str::from_utf8(response.body()).expect("utf-8 envelope");

        let client_error = WebClientError::from_error_response(status, body);

        assert_eq!(status, 401);
        assert_eq!(
            client_error.http_status_code(),
            server_error.http_status_code()
        );
        assert_eq!(client_error.remote_code(), Some(server_error.error_code()));
        assert_eq!(client_error.remote_code(), Some("API_009"));
        assert_eq!(
            client_error.display_message(),
            server_error.display_message()
        );
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn the_servers_refused_fields_round_trip_to_the_client() {
        use fundamental_core::validate::ValidationErrors;
        use fundamental_server::ServerResponse;

        let mut refused = ValidationErrors::new();
        refused.add_for_field("email", "taken", "Someone already has this address.");
        let response = ServerResponse::from_error(&refused);
        let body = std::str::from_utf8(response.body()).expect("utf-8 envelope");

        let client_error = WebClientError::from_error_response(response.status(), body);

        assert_eq!(client_error.field_errors(), refused.errors().to_vec());
    }
}
