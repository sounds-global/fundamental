use std::str::FromStr;
use std::sync::Arc;

use fundamental_server::{Method, NATIVE_WEBVIEW_SCHEME, RequestHeaders, ServerResponse};
use tauri::http::{HeaderName, HeaderValue, Request, Response, header::CONTENT_TYPE};
use tauri::{Builder, Runtime, UriSchemeResponder};
use tracing::error;

use crate::{ActionBridge, BridgeRequest, BridgeResponse};

/// The URI scheme the native webview loads pages from, the framework-wide constant.
pub(crate) const SCHEME: &str = NATIVE_WEBVIEW_SCHEME;

/// The scheme's base URL on this platform.
///
/// WebKit serves `scheme://localhost`; WebView2 and Android serve `http://scheme.localhost`.
pub(crate) fn scheme_base() -> String {
    if cfg!(any(windows, target_os = "android")) {
        format!("http://{SCHEME}.localhost")
    } else {
        format!("{SCHEME}://localhost")
    }
}

pub(crate) fn register<R: Runtime>(
    builder: Builder<R>,
    bridge: Arc<dyn ActionBridge>,
) -> Builder<R> {
    builder.register_asynchronous_uri_scheme_protocol(SCHEME, move |_ctx, request, responder| {
        let bridge = Arc::clone(&bridge);
        tauri::async_runtime::spawn(async move {
            respond(bridge, request, responder).await;
        });
    })
}

async fn respond(
    bridge: Arc<dyn ActionBridge>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let response = handle(bridge, request).await;
    responder.respond(into_http(response));
}

/// Maps a webview request through the bridge into a bridge response.
///
/// Never fails: a failure renders as the standard JSON error envelope.
async fn handle(bridge: Arc<dyn ActionBridge>, request: Request<Vec<u8>>) -> BridgeResponse {
    let method = match Method::from_str(request.method().as_str()) {
        Ok(method) => method,
        Err(unsupported) => return ServerResponse::from_error(&unsupported).into(),
    };
    // Dispatch on the path alone, as the API backend does; the query is still carried.
    let path = request.uri().path().to_owned();
    let query = request.uri().query().unwrap_or_default().to_owned();
    let headers = collect_headers(&request);
    let body = request.into_body();

    match bridge
        .call(
            BridgeRequest::new(method, path)
                .with_query(query)
                .with_headers(headers)
                .with_body(body),
        )
        .await
    {
        Ok(response) => response,
        Err(bridge_error) => {
            // Logged here, once, where the failure is rendered to the webview.
            error!(error = ?bridge_error, "page request failed at the bridge");
            ServerResponse::from_error(&bridge_error).into()
        }
    }
}

/// The request's headers, as the webview actually sent them.
///
/// Nothing is synthesised: this scheme serves fetches too, so a stamped `navigate` would lie. A
/// non-UTF-8 value is skipped rather than lossily converted.
fn collect_headers(request: &Request<Vec<u8>>) -> RequestHeaders {
    request
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_owned(), value.to_owned()))
        })
        .collect()
}

/// Renders a bridge response as the webview's HTTP response.
///
/// The handler's own headers ride along; one the webview refuses is skipped and logged.
fn into_http(response: BridgeResponse) -> Response<Vec<u8>> {
    let mut builder = Response::builder()
        .status(response.status)
        .header(CONTENT_TYPE, response.content_type);
    for (name, value) in response.headers {
        match (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            (Ok(name), Ok(value)) => builder = builder.header(name, value),
            _ => error!(header = %name, "dropping an invalid response header"),
        }
    }
    builder
        .body(response.body)
        // Only an invalid header value can fail, and these content types are valid.
        .unwrap_or_else(|_| {
            Response::builder()
                .status(500)
                .body(Vec::new())
                .unwrap_or_default()
        })
}

#[cfg(test)]
mod tests {
    use super::{SCHEME, into_http, scheme_base};
    use crate::BridgeResponse;

    #[test]
    fn scheme_base_matches_the_platform_mapping() {
        let base = scheme_base();
        if cfg!(any(windows, target_os = "android")) {
            assert_eq!(base, format!("http://{SCHEME}.localhost"));
        } else {
            assert_eq!(base, format!("{SCHEME}://localhost"));
        }
    }

    #[test]
    fn into_http_carries_status_content_type_and_body() {
        let response = into_http(BridgeResponse {
            status: 201,
            content_type: "text/html; charset=utf-8".into(),
            headers: Vec::new(),
            body: b"<p>hi</p>".to_vec(),
        });
        assert_eq!(response.status(), 201);
        assert_eq!(
            response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some("text/html; charset=utf-8")
        );
        assert_eq!(response.body(), &b"<p>hi</p>".to_vec());
    }

    /// A handler's headers reach the webview.
    #[test]
    fn into_http_carries_the_handlers_headers() {
        let response = into_http(BridgeResponse {
            status: 404,
            content_type: "text/html; charset=utf-8".into(),
            headers: vec![("X-Robots-Tag".into(), "noindex".into())],
            body: Vec::new(),
        });
        assert_eq!(response.status(), 404);
        assert_eq!(
            response
                .headers()
                .get("x-robots-tag")
                .and_then(|v| v.to_str().ok()),
            Some("noindex")
        );
    }

    /// One unusable header does not cost the whole page.
    #[test]
    fn into_http_skips_an_invalid_header_and_still_answers() {
        let response = into_http(BridgeResponse {
            status: 200,
            content_type: "text/html; charset=utf-8".into(),
            headers: vec![
                ("not a header name".into(), "x".into()),
                ("X-Ok".into(), "1".into()),
            ],
            body: b"body".to_vec(),
        });
        assert_eq!(response.status(), 200);
        assert!(response.headers().get("x-ok").is_some());
        assert_eq!(response.body(), &b"body".to_vec());
    }
}
