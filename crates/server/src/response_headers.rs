use std::sync::{Arc, Mutex};

/// A per-request, shared sink for response headers a handler wants on its reply, applied on success and error.
#[derive(Debug, Clone, Default)]
pub struct ResponseHeaders {
    // A `Vec` keeps insertion order and repeated names for the backend.
    headers: Arc<Mutex<Vec<(String, String)>>>,
}

impl ResponseHeaders {
    /// Creates an empty sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a response header `(name, value)`, appending rather than replacing.
    pub fn set(&self, name: impl Into<String>, value: impl Into<String>) {
        if let Ok(mut headers) = self.headers.lock() {
            headers.push((name.into(), value.into()));
        }
    }

    /// Records `(name, value)` as the header's only value, case-insensitively.
    pub fn replace(&self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        if let Ok(mut headers) = self.headers.lock() {
            headers.retain(|(recorded, _)| !recorded.eq_ignore_ascii_case(&name));
            headers.push((name, value.into()));
        }
    }

    /// Drains and returns the recorded headers; a poisoned lock yields none.
    #[must_use]
    pub fn take(&self) -> Vec<(String, String)> {
        self.headers
            .lock()
            .map(|mut headers| std::mem::take(&mut *headers))
            .unwrap_or_default()
    }

    /// Returns whether no response headers have been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.headers.lock().map(|h| h.is_empty()).unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_then_take_returns_headers_in_order_and_empties_the_sink() {
        let sink = ResponseHeaders::new();
        assert!(sink.is_empty());
        sink.set("DPoP-Nonce", "abc");
        sink.set("WWW-Authenticate", r#"DPoP error="use_dpop_nonce""#);
        assert!(!sink.is_empty());

        let taken = sink.take();
        assert_eq!(
            taken,
            vec![
                ("DPoP-Nonce".to_owned(), "abc".to_owned()),
                (
                    "WWW-Authenticate".to_owned(),
                    r#"DPoP error="use_dpop_nonce""#.to_owned()
                ),
            ]
        );
        assert!(sink.is_empty());
        assert!(sink.take().is_empty());
    }

    #[test]
    fn a_clone_shares_the_same_sink() {
        let handler_view = ResponseHeaders::new();
        let router_view = handler_view.clone();
        handler_view.set("DPoP-Nonce", "shared");
        assert_eq!(
            router_view.take(),
            vec![("DPoP-Nonce".to_owned(), "shared".to_owned())]
        );
    }

    #[test]
    fn replace_leaves_one_header() {
        let sink = ResponseHeaders::new();
        sink.set("cache-control", "public, max-age=60");
        sink.set("Vary", "Cookie");
        sink.set("Cache-Control", "private");
        sink.replace("Cache-Control", "no-store");
        assert_eq!(
            sink.take(),
            vec![
                ("Vary".to_owned(), "Cookie".to_owned()),
                ("Cache-Control".to_owned(), "no-store".to_owned()),
            ]
        );
    }
}
