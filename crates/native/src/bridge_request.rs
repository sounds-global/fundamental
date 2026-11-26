use fundamental_server::{Method, RequestHeaders};

/// One action call crossing the webview → Rust boundary.
///
/// Carries the request's metadata, so a consumer branching on it sees what was stated.
#[derive(Debug, Clone)]
pub struct BridgeRequest {
    /// The action's HTTP method.
    pub method: Method,
    /// The action's mount path (for example `/api/greet`).
    pub path: String,
    /// The raw query string, without its leading `?`; empty when there is none.
    ///
    /// Routing never consults it, but a handler may.
    pub query: String,
    /// The headers the navigation or the invoke carried.
    pub headers: RequestHeaders,
    /// The serialized input; empty for body-less calls.
    pub body: Vec<u8>,
}

impl BridgeRequest {
    /// Creates a body-less request for `method` and `path`, with no query and no headers.
    pub fn new(method: Method, path: impl Into<String>) -> Self {
        Self {
            method,
            path: path.into(),
            query: String::new(),
            headers: RequestHeaders::new(),
            body: Vec::new(),
        }
    }

    /// Sets the request body bytes.
    #[must_use]
    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    /// Sets the raw query string, with or without a leading `?`.
    ///
    /// The `?` is stripped, as [`ServerRequest::with_query`](fundamental_server::ServerRequest::with_query)
    /// expects.
    #[must_use]
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        let query = query.into();
        self.query = query
            .strip_prefix('?')
            .map_or_else(|| query.clone(), str::to_owned);
        self
    }

    /// Sets the request headers.
    #[must_use]
    pub fn with_headers(mut self, headers: RequestHeaders) -> Self {
        self.headers = headers;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::BridgeRequest;
    use fundamental_server::{Method, RequestHeaders};

    #[test]
    fn new_starts_with_an_empty_body() {
        let request = BridgeRequest::new(Method::Get, "/api/items");
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.path, "/api/items");
        assert!(request.body.is_empty());
    }

    #[test]
    fn with_body_replaces_the_body() {
        let request =
            BridgeRequest::new(Method::Post, "/api/items").with_body(b"{\"a\":1}".to_vec());
        assert_eq!(request.body, b"{\"a\":1}");
    }

    #[test]
    fn new_starts_with_no_query_and_no_headers() {
        let request = BridgeRequest::new(Method::Get, "/api/items");
        assert!(request.query.is_empty());
        assert!(request.headers.get("accept").is_none());
    }

    /// The bare and `?` forms of a query arrive as the same value.
    #[test]
    fn a_leading_question_mark_is_stripped_and_a_bare_query_is_untouched() {
        let with_mark = BridgeRequest::new(Method::Get, "/why").with_query("?tab=files");
        let without = BridgeRequest::new(Method::Get, "/why").with_query("tab=files");
        assert_eq!(with_mark.query, "tab=files");
        assert_eq!(without.query, "tab=files");
    }

    /// Only the leading `?` is a delimiter.
    #[test]
    fn a_question_mark_inside_the_query_is_left_alone() {
        let request =
            BridgeRequest::new(Method::Get, "/login").with_query("next=/reports?tab=files");
        assert_eq!(request.query, "next=/reports?tab=files");
    }

    #[test]
    fn with_headers_replaces_the_headers() {
        let headers: RequestHeaders = [("Sec-Fetch-Mode".to_owned(), "navigate".to_owned())]
            .into_iter()
            .collect();
        let request = BridgeRequest::new(Method::Get, "/").with_headers(headers);
        // Case-insensitively.
        assert_eq!(request.headers.get("sec-fetch-mode"), Some("navigate"));
    }
}
