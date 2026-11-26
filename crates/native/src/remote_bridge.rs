use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use fundamental_server::Method;
use reqwest::Client;
use tracing::{Instrument, debug, info_span};
#[cfg(feature = "client-identity")]
use tracing::{instrument, warn};

use crate::{
    ActionBridge, BridgeRequest, BridgeResponse, NativeError, RequestAuthorizer, TokenSource,
};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// An [`ActionBridge`] that runs actions on a remote API server over HTTPS.
///
/// The remote server verifies the bearer credential itself. A non-2xx reply is still an `Ok`
/// response; only a failed exchange is a [`NativeError`](crate::NativeError).
pub struct RemoteBridge {
    client: Client,
    timeout: Duration,
    base_url: String,
    /// The base URL's origin, which is all a transport failure names.
    origin: String,
    token: Option<Arc<dyn TokenSource>>,
    authorizer: Option<Arc<dyn RequestAuthorizer>>,
}

impl RemoteBridge {
    /// Creates a bridge targeting `base_url`, with a 30-second per-call timeout and no credential.
    ///
    /// # Errors
    /// Returns [`NativeError::InvalidUrl`] if `base_url` is not an absolute
    /// `http`/`https` URL, and `NATIVE_002` if the TLS backend cannot
    /// initialize.
    pub fn new(base_url: impl Into<String>) -> Result<Self, NativeError> {
        Self::with_timeout(base_url, DEFAULT_TIMEOUT)
    }

    /// Creates a bridge with an explicit per-call `timeout` — see [`new`](Self::new).
    ///
    /// # Errors
    /// As [`new`](Self::new).
    pub fn with_timeout(
        base_url: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, NativeError> {
        let base_url = base_url.into();
        let parsed = reqwest::Url::parse(&base_url).map_err(|_| NativeError::InvalidUrl {
            url: base_url.clone(),
        })?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(NativeError::InvalidUrl { url: base_url });
        }
        let origin = parsed.origin().ascii_serialization();
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|source| NativeError::RemoteTransport {
                origin: origin.clone(),
                source: source.without_url(),
            })?;
        Ok(Self {
            client,
            timeout,
            base_url: base_url.trim_end_matches('/').to_owned(),
            origin,
            token: None,
            authorizer: None,
        })
    }

    /// Attaches the bearer-credential source consulted before every call.
    #[must_use]
    pub fn with_token(mut self, source: impl TokenSource) -> Self {
        self.token = Some(Arc::new(source));
        self
    }

    /// Attaches a [`RequestAuthorizer`], so each call carries a credential bound to it.
    #[must_use]
    pub fn with_authorizer(mut self, authorizer: impl RequestAuthorizer) -> Self {
        self.authorizer = Some(Arc::new(authorizer));
        self
    }

    /// Presents `identity`'s certificate on every connection, the server checked against
    /// `trust`; a session cookie stays in the bridge's own jar, never the webview.
    ///
    /// # Errors
    /// [`NativeError::ClientTls`] when `trust` yields no usable root, and `NATIVE_002`
    /// when the client cannot be built.
    #[cfg(feature = "client-identity")]
    #[instrument(skip_all, fields(base_url = %self.base_url), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn with_client_identity(
        mut self,
        identity: Arc<crate::ClientIdentity>,
        trust: &crate::ServerTrust,
    ) -> Result<Self, NativeError> {
        let tls = client_tls(identity, trust)?;
        self.client = Client::builder()
            .timeout(self.timeout)
            .use_preconfigured_tls(tls)
            .cookie_store(true)
            .build()
            .map_err(|source| NativeError::RemoteTransport {
                origin: self.origin.clone(),
                source: source.without_url(),
            })?;
        Ok(self)
    }

    /// Joins the base URL and an action path into the request URL, keeping the query.
    fn url_for(&self, path: &str, query: &str) -> String {
        let base = format!("{}/{}", self.base_url, path.trim_start_matches('/'));
        if query.is_empty() {
            base
        } else {
            format!("{base}?{query}")
        }
    }
}

// Manual, so only the token source's presence is printed.
impl std::fmt::Debug for RemoteBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteBridge")
            .field("base_url", &self.base_url)
            .field("timeout", &self.timeout)
            .field("token", &self.token.is_some())
            .finish_non_exhaustive()
    }
}

impl ActionBridge for RemoteBridge {
    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    fn call(
        &self,
        request: BridgeRequest,
    ) -> Pin<Box<dyn Future<Output = Result<BridgeResponse, NativeError>> + Send + '_>> {
        let url = self.url_for(&request.path, &request.query);
        // No path: it is the webview's.
        let span = info_span!(
            "native.invoke",
            bridge = "remote",
            method = %request.method,
            status = tracing::field::Empty,
        );

        Box::pin(
            async move {
                let mut builder = self
                    .client
                    .request(to_reqwest_method(request.method), &url)
                    .header(reqwest::header::CONTENT_TYPE, "application/json");

                // The host's own headers are never forwarded, so page script cannot choose
                // the credential this process presents.
                for (name, value) in request.headers.iter() {
                    if is_the_hosts_to_set(name) {
                        continue;
                    }
                    builder = builder.header(name, value);
                }

                builder = builder.body(request.body);

                let token = match &self.token {
                    Some(source) => {
                        let token = source
                            .token()
                            .await
                            .map_err(|source| NativeError::Token { source })?;
                        debug!(authenticated = token.is_some(), "bearer token resolved");
                        token
                    }
                    None => None,
                };
                if let Some(token) = &token {
                    builder = builder.bearer_auth(token.expose());
                }

                // Last, so it takes precedence.
                if let Some(authorizer) = &self.authorizer {
                    let headers = authorizer
                        .authorize(request.method, &url, token.as_ref())
                        .await
                        .map_err(|source| NativeError::Token { source })?;
                    for (name, value) in headers {
                        builder = builder.header(name, value);
                    }
                }

                let response =
                    builder
                        .send()
                        .await
                        .map_err(|source| NativeError::RemoteTransport {
                            origin: self.origin.clone(),
                            source: source.without_url(),
                        })?;

                let status = response.status().as_u16();
                tracing::Span::current().record("status", status);
                let content_type = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("application/json")
                    .to_owned();
                // Every header but the content type, skipping non-UTF-8 values. `Set-Cookie`
                // is dropped here, so a remote origin cannot put a cookie in the app's jar.
                let headers: Vec<(String, String)> = response
                    .headers()
                    .iter()
                    .filter(|(name, _)| *name != reqwest::header::CONTENT_TYPE)
                    .filter(|(name, _)| *name != reqwest::header::SET_COOKIE)
                    .filter_map(|(name, value)| {
                        value
                            .to_str()
                            .ok()
                            .map(|value| (name.as_str().to_owned(), value.to_owned()))
                    })
                    .collect();
                let body = response
                    .bytes()
                    .await
                    .map_err(|source| NativeError::RemoteTransport {
                        origin: self.origin.clone(),
                        source: source.without_url(),
                    })?
                    .to_vec();

                Ok(BridgeResponse {
                    status,
                    content_type,
                    headers,
                    body,
                })
            }
            .instrument(span),
        )
    }
}

/// A TLS client configuration trusting `trust` and presenting `identity`.
#[cfg(feature = "client-identity")]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn client_tls(
    identity: Arc<crate::ClientIdentity>,
    trust: &crate::ServerTrust,
) -> Result<rustls::ClientConfig, NativeError> {
    use rustls::pki_types::pem::PemObject as _;

    let failed = |detail: String| NativeError::ClientTls { detail };
    let certificates = match trust {
        crate::ServerTrust::Platform => {
            let native = rustls_native_certs::load_native_certs();
            for error in &native.errors {
                warn!(%error, "a trust store entry could not be read, and is not trusted");
            }
            native.certs
        }
        crate::ServerTrust::Anchors(pem) => {
            rustls::pki_types::CertificateDer::pem_slice_iter(pem.as_bytes())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| failed(format!("the pinned anchors do not parse: {error}")))?
        }
    };
    let mut roots = rustls::RootCertStore::empty();
    let (added, ignored) = roots.add_parsable_certificates(certificates);
    if ignored > 0 {
        warn!(ignored, "roots that did not parse are not trusted");
    }
    if added == 0 {
        return Err(failed("the server trust holds no usable root".to_owned()));
    }
    debug!(
        roots = added,
        pinned = matches!(trust, crate::ServerTrust::Anchors(_)),
        "server roots loaded"
    );
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|error| failed(error.to_string()))?
    .with_root_certificates(roots)
    .with_client_cert_resolver(identity);
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    // No resumption, so every connection is a full handshake presenting the current chain.
    config.resumption = rustls::client::Resumption::disabled();
    Ok(config)
}

/// Whether a header belongs to the host, so it is never forwarded from the webview.
fn is_the_hosts_to_set(name: &str) -> bool {
    const HOSTS: [&str; 8] = [
        "authorization",
        "dpop",
        "host",
        "content-length",
        "content-type",
        "connection",
        "transfer-encoding",
        "cookie",
    ];
    HOSTS.iter().any(|denied| name.eq_ignore_ascii_case(denied))
}

fn to_reqwest_method(method: Method) -> reqwest::Method {
    match method {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
        Method::Delete => reqwest::Method::DELETE,
        // `Method` is `#[non_exhaustive]`; unreachable, but must not panic.
        _ => reqwest::Method::GET,
    }
}

#[cfg(test)]
mod tests {
    use super::RemoteBridge;
    use crate::{ActionBridge, BridgeRequest, NativeError, TokenSource};
    use fundamental_core::AnyError;
    use fundamental_core::secrets::SecretString;
    use fundamental_server::Method;
    use std::future::Future;
    use std::pin::Pin;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn one_shot_server(
        status_line: &'static str,
        content_type: &'static str,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = socket.read(&mut buf).await.expect("read");
                request.extend_from_slice(&buf[..n]);
                if request.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let response = format!(
                "{status_line}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.expect("write");
            socket.shutdown().await.ok();
            String::from_utf8_lossy(&request).into_owned()
        });
        (format!("http://{addr}"), handle)
    }

    /// Serves one exchange whose response carries `extra` verbatim before the body.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn one_shot_server_sending(
        extra: &'static str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = socket.read(&mut buf).await.expect("read");
                request.extend_from_slice(&buf[..n]);
                if request.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let body = "{}";
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n{extra}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.expect("write");
            socket.shutdown().await.ok();
            String::from_utf8_lossy(&request).into_owned()
        });
        (format!("http://{addr}"), handle)
    }

    struct FixedToken;
    impl TokenSource for FixedToken {
        fn token(
            &self,
        ) -> Pin<Box<dyn Future<Output = Result<Option<SecretString>, AnyError>> + Send + '_>>
        {
            Box::pin(async { Ok(Some(SecretString::new("tok-123".to_string()))) })
        }
    }

    /// A failed exchange names the configured origin and nothing of the call.
    #[tokio::test]
    async fn a_transport_failure_carries_the_origin_and_never_the_request() {
        let closed = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let address = closed.local_addr().expect("an address");
        drop(closed);
        let bridge = RemoteBridge::new(format!("http://{address}/base")).expect("a valid base");
        let error = bridge
            .call(
                BridgeRequest::new(Method::Get, "/reset/jan@example.nl")
                    .with_query("email=jan@example.nl"),
            )
            .await
            .expect_err("nothing listens there");
        assert!(matches!(error, NativeError::RemoteTransport { .. }));
        let mut rendered = format!("{error} {error:?}");
        let mut source = std::error::Error::source(&error);
        while let Some(cause) = source {
            rendered.push_str(&format!(" {cause} {cause:?}"));
            source = cause.source();
        }
        assert!(
            rendered.contains(&format!("http://{address}")),
            "{rendered}"
        );
        assert!(!rendered.contains("jan"), "{rendered}");
        assert!(!rendered.contains("/base"), "{rendered}");
    }

    #[tokio::test]
    async fn a_remote_set_cookie_never_reaches_the_response() {
        let (base, server) =
            one_shot_server_sending("set-cookie: __Host-session=stolen; Path=/; Secure").await;
        let bridge = RemoteBridge::new(&base).expect("bridge");
        let response = bridge
            .call(BridgeRequest::new(Method::Get, "/whoami"))
            .await
            .expect("call");
        server.await.expect("server");

        assert!(
            !response
                .headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("set-cookie")),
            "a remote origin must not be able to set a cookie in a native app's jar"
        );
    }

    #[tokio::test]
    async fn an_ordinary_remote_header_still_reaches_the_response() {
        let (base, server) = one_shot_server_sending("x-request-id: abc123").await;
        let bridge = RemoteBridge::new(&base).expect("bridge");
        let response = bridge
            .call(BridgeRequest::new(Method::Get, "/whoami"))
            .await
            .expect("call");
        server.await.expect("server");

        assert!(
            response
                .headers
                .iter()
                .any(|(name, value)| name.eq_ignore_ascii_case("x-request-id")
                    && value == "abc123"),
            "the filter must be narrow enough to leave ordinary headers alone"
        );
    }

    struct FailingToken;
    impl TokenSource for FailingToken {
        fn token(
            &self,
        ) -> Pin<Box<dyn Future<Output = Result<Option<SecretString>, AnyError>> + Send + '_>>
        {
            Box::pin(async {
                #[derive(Debug, fundamental_macros::Error)]
                enum StoreError {
                    #[error(code = "STORE_002", message = "refresh failed", status = 500)]
                    Refresh,
                }
                Err(AnyError::new(StoreError::Refresh))
            })
        }
    }

    #[tokio::test]
    async fn forwards_method_path_body_and_returns_the_response() {
        let (url, server) =
            one_shot_server("HTTP/1.1 200 OK", "application/json", "{\"ok\":true}").await;
        let bridge = RemoteBridge::new(&url).expect("valid url");
        let response = bridge
            .call(BridgeRequest::new(Method::Post, "/api/echo").with_body(b"{\"n\":1}".to_vec()))
            .await
            .expect("reachable");
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json");
        assert_eq!(response.body, b"{\"ok\":true}");

        let request = server.await.expect("server task");
        assert!(request.starts_with("POST /api/echo HTTP/1.1"));
        assert!(request.contains("content-type: application/json"));
    }

    #[tokio::test]
    async fn a_non_2xx_reply_is_a_response_not_an_error() {
        let (url, _server) = one_shot_server(
            "HTTP/1.1 404 Not Found",
            "application/json",
            "{\"error\":\"API_002\"}",
        )
        .await;
        let bridge = RemoteBridge::new(&url).expect("valid url");
        let response = bridge
            .call(BridgeRequest::new(Method::Get, "/api/missing"))
            .await
            .expect("a 404 travels as a response");
        assert_eq!(response.status, 404);
        assert!(!response.is_success());
    }

    #[tokio::test]
    async fn attaches_the_bearer_token_when_a_source_is_wired() {
        let (url, server) = one_shot_server("HTTP/1.1 200 OK", "application/json", "{}").await;
        let bridge = RemoteBridge::new(&url)
            .expect("valid url")
            .with_token(FixedToken);
        bridge
            .call(BridgeRequest::new(Method::Get, "/api/items"))
            .await
            .expect("reachable");
        let request = server.await.expect("server task");
        assert!(request.contains("authorization: Bearer tok-123"));
    }

    #[tokio::test]
    async fn a_failing_token_source_aborts_with_native_004() {
        use fundamental_core::Error as _;
        let bridge = RemoteBridge::new("http://127.0.0.1:9")
            .expect("valid url")
            .with_token(FailingToken);
        let err = bridge
            .call(BridgeRequest::new(Method::Get, "/api/items"))
            .await
            .expect_err("token failure aborts the call");
        assert!(matches!(err, NativeError::Token { .. }));
        assert_eq!(err.error_code(), "NATIVE_004");
    }

    #[tokio::test]
    async fn an_unreachable_server_is_a_retryable_transport_error() {
        use fundamental_core::Error as _;
        let bridge = RemoteBridge::new("http://127.0.0.1:9").expect("valid url");
        let err = bridge
            .call(BridgeRequest::new(Method::Get, "/api/items"))
            .await
            .expect_err("nothing listens on discard");
        assert!(matches!(err, NativeError::RemoteTransport { .. }));
        assert_eq!(err.error_code(), "NATIVE_002");
        assert!(err.is_retryable());
    }

    #[test]
    fn rejects_a_relative_or_non_http_base_url() {
        use fundamental_core::Error as _;
        for bad in ["not a url", "/relative", "ftp://host/api"] {
            let err = RemoteBridge::new(bad).expect_err("must reject");
            assert!(matches!(err, NativeError::InvalidUrl { .. }), "{bad}");
            assert_eq!(err.error_code(), "NATIVE_003");
        }
    }

    #[test]
    fn url_for_joins_without_doubling_slashes() {
        let bridge = RemoteBridge::new("http://api.example.com/").expect("valid url");
        assert_eq!(
            bridge.url_for("/api/echo", ""),
            "http://api.example.com/api/echo"
        );
        assert_eq!(
            bridge.url_for("api/echo", ""),
            "http://api.example.com/api/echo"
        );
    }

    #[test]
    fn url_for_carries_the_requests_own_query() {
        let bridge = RemoteBridge::new("http://api.example.com").expect("valid url");
        assert_eq!(
            bridge.url_for("/login", "next=%2Freports&lang=nl"),
            "http://api.example.com/login?next=%2Freports&lang=nl"
        );
    }

    #[tokio::test]
    async fn a_bridged_request_states_its_headers_and_query_to_the_remote() {
        let (base, server) = one_shot_server("HTTP/1.1 200 OK", "application/json", "{}").await;
        let bridge = RemoteBridge::new(&base).expect("bridge");
        let headers: fundamental_server::RequestHeaders =
            [("accept-language".to_string(), "nl-NL".to_string())]
                .into_iter()
                .collect();
        bridge
            .call(
                BridgeRequest::new(Method::Get, "/login")
                    .with_query("next=%2Freports")
                    .with_headers(headers),
            )
            .await
            .expect("call");
        let raw = server.await.expect("server");

        assert!(raw.contains("/login?next=%2Freports"), "{raw}");
        assert!(
            raw.to_ascii_lowercase()
                .contains("accept-language: nl-NL".to_ascii_lowercase().as_str()),
            "{raw}"
        );
    }

    #[tokio::test]
    async fn a_bridged_request_cannot_choose_the_credential_the_host_presents() {
        let (base, server) = one_shot_server("HTTP/1.1 200 OK", "application/json", "{}").await;
        let bridge = RemoteBridge::new(&base)
            .expect("bridge")
            .with_token(FixedToken);
        let headers: fundamental_server::RequestHeaders = [
            (
                "authorization".to_string(),
                "Bearer attacker-chosen".to_string(),
            ),
            ("dpop".to_string(), "attacker-proof".to_string()),
        ]
        .into_iter()
        .collect();
        bridge
            .call(BridgeRequest::new(Method::Post, "/act").with_headers(headers))
            .await
            .expect("call");
        let raw = server.await.expect("server");

        assert!(
            raw.contains("Bearer tok-123"),
            "the host's own token must be the one sent: {raw}"
        );
        assert!(
            !raw.contains("attacker-chosen"),
            "a bridged request must not choose the Authorization header: {raw}"
        );
        assert!(
            !raw.to_ascii_lowercase().contains("dpop:"),
            "a bridged request must not choose the DPoP header: {raw}"
        );
    }
}
