/// The URI scheme the native surface's webview serves its in-process pages from.
pub const NATIVE_WEBVIEW_SCHEME: &str = "fundamental";

/// Whether `origin` may be granted the development-only cross-origin allowance.
///
/// A loopback `http` origin on any port, or the native webview's own origins; the host is
/// matched exactly.
#[must_use]
pub fn is_dev_tools_origin(origin: &str) -> bool {
    is_http_loopback(origin) || is_native_webview(origin)
}

/// Whether `origin` is one of the native webview's own origins (exact host match).
fn is_native_webview(origin: &str) -> bool {
    // WebKit (macOS/iOS): `fundamental://localhost`.
    if let Some(rest) = origin.strip_prefix(NATIVE_WEBVIEW_SCHEME)
        && rest == "://localhost"
    {
        return true;
    }
    // WebView2 (Windows) / Android: `http://fundamental.localhost` (any port).
    http_host(origin)
        .is_some_and(|host| host.strip_prefix(NATIVE_WEBVIEW_SCHEME) == Some(".localhost"))
}

/// Whether `origin` is a loopback `http` origin, on any port.
fn is_http_loopback(origin: &str) -> bool {
    matches!(http_host(origin), Some("127.0.0.1" | "localhost" | "::1"))
}

/// The host of an `http://` origin, its port stripped; `None` for a non-`http` origin.
fn http_host(origin: &str) -> Option<&str> {
    let rest = origin.strip_prefix("http://")?;
    match rest.strip_prefix('[') {
        Some(v6) => v6.split_once(']').map(|(inner, _)| inner),
        None => Some(rest.split(':').next().unwrap_or(rest)),
    }
}

#[cfg(test)]
mod tests {
    use super::is_dev_tools_origin;

    #[test]
    fn permits_loopback_http_origins_on_any_port() {
        for origin in [
            "http://127.0.0.1:4000",
            "http://localhost:4000",
            "http://127.0.0.1",
            "http://localhost",
            "http://[::1]:9000",
        ] {
            assert!(is_dev_tools_origin(origin), "{origin} should be allowed");
        }
    }

    #[test]
    fn permits_the_native_webview_origins() {
        for origin in [
            "fundamental://localhost",
            "http://fundamental.localhost",
            "http://fundamental.localhost:1420",
        ] {
            assert!(is_dev_tools_origin(origin), "{origin} should be allowed");
        }
    }

    #[test]
    fn rejects_remote_and_look_alike_origins() {
        for origin in [
            "https://127.0.0.1:4000", // not plain http
            "http://evil.com",
            "http://127.0.0.1.evil.com", // host is not exactly 127.0.0.1
            "http://localhost.evil.com",
            "http://fundamental.localhost.evil.com", // host is not exactly fundamental.localhost
            "fundamental://localhost.evil.com",      // not exactly fundamental://localhost
            "evil://localhost",
            "http://10.0.0.1:4000",
            "",
        ] {
            assert!(!is_dev_tools_origin(origin), "{origin} must be rejected");
        }
    }
}
