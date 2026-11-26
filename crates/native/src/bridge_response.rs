use fundamental_server::ServerResponse;

/// The result of a bridged action call: status, headers, content type, and body.
///
/// Mirrors [`ServerResponse`], so a response reads the same in-process or remote; `headers`
/// keeps what the handler recorded.
#[derive(Debug, Clone)]
pub struct BridgeResponse {
    /// The HTTP status code.
    pub status: u16,
    /// The content type of `body`.
    pub content_type: String,
    /// Additional response headers the handler recorded, in the order it set them.
    pub headers: Vec<(String, String)>,
    /// The response body bytes.
    pub body: Vec<u8>,
}

impl BridgeResponse {
    /// Returns whether the status is a success (2xx).
    #[must_use]
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

impl From<ServerResponse> for BridgeResponse {
    fn from(response: ServerResponse) -> Self {
        let status = response.status();
        let content_type = response.content_type().to_owned();
        let headers = response.headers().to_vec();
        Self {
            status,
            content_type,
            headers,
            body: response.into_body(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BridgeResponse;
    use fundamental_server::ServerResponse;

    #[test]
    fn from_server_response_keeps_status_content_type_and_body() {
        let response: BridgeResponse = ServerResponse::html(201, b"<p>ok</p>".to_vec()).into();
        assert_eq!(response.status, 201);
        assert_eq!(response.content_type, "text/html; charset=utf-8");
        assert_eq!(response.body, b"<p>ok</p>");
        assert!(response.headers.is_empty());
    }

    /// A header the handler records survives the bridge.
    #[test]
    fn from_server_response_keeps_the_handlers_headers_in_order() {
        let response: BridgeResponse = ServerResponse::html(200, Vec::new())
            .with_header("X-Robots-Tag", "noindex")
            .with_header("Link", "</a.css>; rel=preload")
            .into();
        assert_eq!(
            response.headers,
            [
                ("X-Robots-Tag".to_owned(), "noindex".to_owned()),
                ("Link".to_owned(), "</a.css>; rel=preload".to_owned()),
            ]
        );
    }

    #[test]
    fn is_success_covers_exactly_the_2xx_range() {
        let mut response: BridgeResponse = ServerResponse::new(200, Vec::new()).into();
        assert!(response.is_success());
        response.status = 299;
        assert!(response.is_success());
        response.status = 199;
        assert!(!response.is_success());
        response.status = 404;
        assert!(!response.is_success());
    }
}
