//! [`AxumBackend`] — the axum implementation of the [`ServerBackend`] seam; no axum type escapes it.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;

use axum::body::{Body, HttpBody};
use axum::extract::{FromRequestParts, Request, State};
use axum::http::header::{
    ACCEPT_LANGUAGE, ACCESS_CONTROL_REQUEST_METHOD, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE,
    COOKIE, HOST, IF_NONE_MATCH,
};
use axum::http::request::Parts;
use axum::http::uri::Authority;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method as HttpMethod, StatusCode, Uri};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::Response;
use fundamental_core::i18n::Locale;
#[cfg(unix)]
use fundamental_core::listen::ActivatedSockets;
use fundamental_core::listen::{Listen, SocketName};
use fundamental_core::{AnyError, Error};
use fundamental_server::{
    AdmissionRequest, BodyPolicy, Method, Peer, PresentedCredential, RequestBodyStream,
    RequestHeaders, RequestIdResolution, Router, ServerBackend, ServerConfig, ServerError,
    ServerRequest, ServerResponse, guard_cookie_write, guard_cross_origin_read,
    is_dev_tools_origin,
};
use http_body_util::{BodyExt, LengthLimitError, Limited};
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::UnixListener;
use tokio::sync::oneshot;
use tower::ServiceBuilder;
use tower::util::MapRequestLayer;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::request_id::PropagateRequestIdLayer;
use tower_http::sensitive_headers::SetSensitiveRequestHeadersLayer;
use tower_http::trace::TraceLayer;
use tracing::{debug, info, warn};

use crate::api_listener::ApiListener;
use crate::boxed_verifier::BoxedVerifier;
use crate::guarded_body::GuardedBody;
use crate::inbound_trace::InboundTrace;
use crate::panic_response::panic_response;
use crate::request_span::request_span;
use crate::resolve_request_id::resolve_request_id;
use crate::security_header_layer::SecurityHeaderLayer;
use crate::serve_connections::serve_connections;

/// The axum-backed HTTP server backend, normally used through [`ApiServer`](crate::ApiServer).
#[derive(Default, Clone)]
pub struct AxumBackend {
    verifier: Option<Arc<dyn BoxedVerifier>>,
}

impl AxumBackend {
    /// Creates a backend with no principal verifier (every request is anonymous).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn with_verifier(verifier: Option<Arc<dyn BoxedVerifier>>) -> Self {
        Self { verifier }
    }
}

impl ServerBackend for AxumBackend {
    type Listener = ApiListener;

    async fn serve<S>(
        &self,
        config: ServerConfig,
        state: S,
        router: Arc<Router<S>>,
    ) -> Result<Self::Listener, AnyError>
    where
        S: Clone + Send + Sync + 'static,
    {
        let policy = config.security_headers().cloned().unwrap_or_default();
        let security_headers = SecurityHeaderLayer::new(&policy).map_err(AnyError::new)?;
        let stopping = Arc::clone(&router);
        let app = build_app(
            &config,
            &security_headers,
            state,
            router,
            self.verifier.clone(),
        );

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let shutdown = async move {
            let _ = shutdown_rx.await;
            // A graceful stop waits for every response, and a held-open page stream never finishes.
            stopping.end_open_streams();
        };
        let timeouts = (config.header_read_timeout(), config.write_stall_timeout());
        let (local_addr, task) = match config.listen() {
            Listen::Address(address) => {
                let listener = bound(*address).await?;
                let local_addr = listener
                    .local_addr()
                    .map_err(|e| bind_failed(*address, &e))?;
                let task = tokio::spawn(serve_connections(
                    listener, app, timeouts.0, timeouts.1, shutdown,
                ));
                (Listen::Address(local_addr), task)
            }
            Listen::Activated(name) => {
                let listener = activated(name)?;
                let task = tokio::spawn(serve_connections(
                    listener, app, timeouts.0, timeouts.1, shutdown,
                ));
                (Listen::Activated(name.clone()), task)
            }
        };

        info!(addr = %local_addr, "api server listening");
        Ok(ApiListener::new(local_addr, shutdown_tx, task))
    }
}

async fn bound(address: SocketAddr) -> Result<TcpListener, AnyError> {
    TcpListener::bind(address)
        .await
        .map_err(|e| AnyError::new(bind_failed(address, &e)))
}

fn bind_failed(address: SocketAddr, error: &std::io::Error) -> ServerError {
    ServerError::Bind {
        address: address.to_string(),
        message: error.to_string(),
    }
}

/// The socket the service manager passed as `name`, ready for the runtime.
#[cfg(unix)]
fn activated(name: &SocketName) -> Result<UnixListener, AnyError> {
    let unavailable = |reason: String| {
        AnyError::new(ServerError::ActivatedSocketUnavailable {
            name: name.to_string(),
            reason,
        })
    };
    let listener = ActivatedSockets::inherited()
        .take(name)
        .map_err(|fault| unavailable(fault.to_string()))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| unavailable(e.to_string()))?;
    UnixListener::from_std(listener).map_err(|e| unavailable(e.to_string()))
}

#[cfg(not(unix))]
fn activated(name: &SocketName) -> Result<TcpListener, AnyError> {
    Err(AnyError::new(ServerError::ActivatedSocketUnavailable {
        name: name.to_string(),
        reason: "passed sockets are served only on unix".to_owned(),
    }))
}

/// The state threaded through every request: the app's state, the router, the verifier, the config.
struct AppState<S> {
    state: S,
    router: Arc<Router<S>>,
    verifier: Option<Arc<dyn BoxedVerifier>>,
    config: Arc<ServerConfig>,
}

// Derived `Clone` would demand `S: Clone` on the struct.
impl<S: Clone> Clone for AppState<S> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            router: Arc::clone(&self.router),
            verifier: self.verifier.clone(),
            config: Arc::clone(&self.config),
        }
    }
}

/// Builds the axum app: a catch-all to the neutral router, with the core middleware over it.
fn build_app<S>(
    config: &ServerConfig,
    security_headers: &SecurityHeaderLayer,
    state: S,
    router: Arc<Router<S>>,
    verifier: Option<Arc<dyn BoxedVerifier>>,
) -> axum::Router
where
    S: Clone + Send + Sync + 'static,
{
    let config = Arc::new(config.clone());
    let middleware = ServiceBuilder::new()
        // Outermost, so every response below, a caught panic or a refusal included, carries the policy.
        .layer(security_headers.layer())
        .layer(SetSensitiveRequestHeadersLayer::new([
            AUTHORIZATION,
            COOKIE,
        ]))
        // Outermost of everything that reads the id, so none sees a caller's value unless adopted.
        .layer(MapRequestLayer::new({
            let ids = config.inbound_request_id();
            let traces = config.inbound_trace_context();
            move |request| InboundTrace::attach(resolve_request_id(request, ids), traces)
        }))
        .layer(PropagateRequestIdLayer::x_request_id())
        // Not the default span, which records the caller's full URI.
        .layer(TraceLayer::new_for_http().make_span_with(request_span))
        .layer(CatchPanicLayer::custom(panic_response))
        // Outside CORS, which answers a preflight itself.
        .layer(from_fn_with_state(
            Arc::clone(&config),
            refuse_unserved_authority,
        ))
        // No global body limit or timeout: each route's own policy applies in `dispatch`.
        .layer(cors_layer(&config, Arc::clone(&router)));

    axum::Router::new()
        .fallback(dispatch::<S>)
        .layer(middleware)
        .with_state(AppState {
            state,
            router,
            verifier,
            config,
        })
}

/// Builds the CORS layer, allowing each cross-origin route's own origins on that route alone.
fn cors_layer<S>(config: &ServerConfig, router: Arc<Router<S>>) -> CorsLayer
where
    S: Send + Sync + 'static,
{
    // An empty or unparseable set falls back to the baseline.
    let mut allowed_headers: Vec<HeaderName> = config
        .cors_allowed_headers()
        .iter()
        .filter_map(|header| HeaderName::from_str(header).ok())
        .collect();
    if allowed_headers.is_empty() {
        allowed_headers = vec![CONTENT_TYPE, AUTHORIZATION];
    }

    // A header not exposed is hidden from cross-origin JS even when sent.
    let exposed_headers: Vec<HeaderName> = config
        .cors_exposed_headers()
        .iter()
        .filter_map(|header| HeaderName::from_str(header).ok())
        .collect();

    let cors = CorsLayer::new()
        .allow_methods([
            HttpMethod::GET,
            HttpMethod::HEAD,
            HttpMethod::POST,
            HttpMethod::PUT,
            HttpMethod::PATCH,
            HttpMethod::DELETE,
        ])
        .allow_headers(allowed_headers)
        .expose_headers(exposed_headers);

    let origins: Vec<HeaderValue> = config
        .cors_allowed_origins()
        .iter()
        .filter_map(|origin| origin.parse().ok())
        .collect();

    let dev_tools = config.allow_dev_tools_cors();
    if !dev_tools && router.cross_origin_routes().is_empty() {
        return if origins.is_empty() {
            cors
        } else {
            cors.allow_origin(origins)
        };
    }

    // Dev-only (`fun dev`): any loopback origin or the native tool's webview scheme.
    let config = config.clone();
    cors.allow_origin(AllowOrigin::predicate(
        move |origin: &HeaderValue, request: &Parts| {
            (dev_tools && is_dev_tools_origin(origin.to_str().unwrap_or_default()))
                || origins.iter().any(|allowed| allowed == origin)
                || cross_origin_allows(&config, &router, origin, request)
        },
    ))
}

/// Whether the route `request` reaches answers `origin`; a preflight by its announced method.
fn cross_origin_allows<S>(
    config: &ServerConfig,
    router: &Router<S>,
    origin: &HeaderValue,
    request: &Parts,
) -> bool
where
    S: Send + Sync + 'static,
{
    let announced = if request.method == HttpMethod::OPTIONS {
        request
            .headers
            .get(ACCESS_CONTROL_REQUEST_METHOD)
            .and_then(|method| method.to_str().ok())
    } else {
        Some(request.method.as_str())
    };
    let method = match announced {
        Some("HEAD") => Ok(Method::Get),
        Some(method) => Method::from_str(method),
        None => return false,
    };
    method
        .ok()
        .and_then(|method| router.cross_origin(method, request.uri.path()))
        .and_then(|set| config.cross_origins(set))
        .is_some_and(|allowed| allowed.iter().any(|allowed| allowed == origin))
}

/// The peer a request's connection came from, or `None` when driven as a bare `tower` service.
#[derive(Debug, Clone, Copy)]
struct PeerAddr(Option<Peer>);

impl<S> FromRequestParts<S> for PeerAddr
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Infallible> {
        Ok(Self(parts.extensions.get::<Peer>().copied()))
    }
}

/// The request id [`resolve_request_id`] decided; minted here if that layer did not run.
#[derive(Debug, Clone, Copy)]
struct RequestIdDecided(RequestIdResolution);

impl<S> FromRequestParts<S> for RequestIdDecided
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Infallible> {
        Ok(Self(
            parts
                .extensions
                .get::<RequestIdResolution>()
                .copied()
                .unwrap_or_else(RequestIdResolution::minted),
        ))
    }
}

/// The catch-all handler: a neutral [`ServerRequest`] dispatched and its [`ServerResponse`] rendered.
#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "readability baseline"
)]
async fn dispatch<S>(
    State(app): State<AppState<S>>,
    PeerAddr(peer): PeerAddr,
    RequestIdDecided(request_id): RequestIdDecided,
    request: Request,
) -> Response
where
    S: Clone + Send + Sync + 'static,
{
    // `HEAD` is routed as `GET` so the two can never disagree (RFC 9110 §9.3.2); the body is
    // dropped afterwards so the `GET` headers, `Content-Length` above all, are kept.
    let (mut parts, body) = request.into_parts();
    let uri = parts.uri.clone();
    // Taken rather than cloned: the body read below needs only the extensions.
    let mut headers = std::mem::take(&mut parts.headers);
    let is_head = parts.method == HttpMethod::HEAD;
    let routed = if is_head {
        HttpMethod::GET
    } else {
        parts.method.clone()
    };
    let our_method = Method::from_str(routed.as_str());
    let method_seen = our_method.as_ref().ok().copied();

    // A cross-origin route reads no credential, so no browser's session can act through it.
    let cross_origin = method_seen
        .and_then(|method| app.router.cross_origin(method, uri.path()))
        .map(|set| app.config.cross_origins(set).unwrap_or_default());
    if cross_origin.is_some() {
        headers.remove(COOKIE);
        headers.remove(AUTHORIZATION);
    }
    let neutral = neutral_headers(&headers);

    // A declared public file at its exact version is answered before admission; it varies on nothing.
    if routed == HttpMethod::GET
        && let Some(answer) = app.router.addressed_answer(
            uri.path(),
            uri.query().unwrap_or(""),
            addressed_authority(&headers, &uri),
            headers
                .get(IF_NONE_MATCH)
                .and_then(|value| value.to_str().ok()),
        )
    {
        debug!(
            status = answer.status(),
            "answered an addressed file before admission"
        );
        let rendered = into_axum(answer);
        return if is_head {
            without_body(rendered)
        } else {
            rendered
        };
    }

    // Admission runs before the body is read or a credential verified.
    if let Some(admission) = app.config.admission()
        && let Err(refusal) = admission.admit(&AdmissionRequest::new(
            peer,
            our_method.as_ref().ok().copied(),
            &neutral,
        ))
    {
        let refusal = refusal.get();
        debug!(
            error = refusal.error_code(),
            status = refusal.http_status_code(),
            "request refused at admission"
        );
        return refused(
            &app,
            ServerResponse::from_error(refusal),
            method_seen,
            &headers,
            &uri,
            request_id,
            is_head,
        )
        .await;
    }

    let our_method = match our_method {
        Ok(method) => method,
        Err(unsupported) => {
            return refused(
                &app,
                ServerResponse::from_error(&unsupported),
                method_seen,
                &headers,
                &uri,
                request_id,
                is_head,
            )
            .await;
        }
    };

    // An ambiguous credential (one cookie name, two values) is refused rather than guessed.
    let presented = match app.config.credential_source().select(
        headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok()),
        cookie_fields(&headers),
    ) {
        Ok(presented) => presented,
        Err(error) => {
            return refused(
                &app,
                ServerResponse::from_error(&error),
                method_seen,
                &headers,
                &uri,
                request_id,
                is_head,
            )
            .await;
        }
    };

    // A cookie is ambient authority, so a write on a cookie surface must prove same origin, checked
    // before verification. A request presenting nothing is guarded too: a forged cross-site sign-in
    // would plant the attacker's session. A bearer is exempt: no browser attaches one unprompted.
    let from_cookie = presented
        .as_ref()
        .is_some_and(PresentedCredential::is_cookie);
    // A cross-origin route's write is guarded too, trusting only its own origin set.
    let carries_ambient_authority =
        from_cookie || (presented.is_none() && app.config.credential_source().reads_cookie());
    if (carries_ambient_authority || cross_origin.is_some())
        && let Err(error) = guard_cookie_write(
            our_method,
            &neutral,
            addressed_authority(&headers, &uri),
            cross_origin.unwrap_or_else(|| app.config.trusted_write_origins()),
        )
    {
        return refused(
            &app,
            ServerResponse::from_error(&error),
            method_seen,
            &headers,
            &uri,
            request_id,
            is_head,
        )
        .await;
    }
    // Its reads too, when a browser says another page sent them.
    if let Some(origins) = cross_origin
        && let Err(error) = guard_cross_origin_read(our_method, &neutral, origins)
    {
        return refused(
            &app,
            ServerResponse::from_error(&error),
            method_seen,
            &headers,
            &uri,
            request_id,
            is_head,
        )
        .await;
    }

    // A cookie the verifier judges bad (a non-retryable `401`) is treated as absent and cleared, or
    // a dead session would lock the person out of sign-in. Any other failure keeps the cookie.
    let mut clear_cookie = None;
    let principal = match (&app.verifier, presented) {
        (Some(verifier), Some(credential)) => match match tokio::time::timeout(
            app.config.request_timeout(),
            verifier.verify(credential.token()),
        )
        .await
        {
            Ok(verified) => verified,
            Err(_) => {
                warn!(
                    source = credential.source_label(),
                    "credential verification ran past the request timeout"
                );
                return refused(
                    &app,
                    ServerResponse::from_error(&ServerError::RequestTimedOut),
                    method_seen,
                    &headers,
                    &uri,
                    request_id,
                    is_head,
                )
                .await;
            }
        } {
            Ok(principal) => Some(principal),
            Err(error)
                if credential.is_cookie()
                    && !error.is_retryable()
                    && error.http_status_code() == 401 =>
            {
                info!(
                    error = error.error_code(),
                    source = credential.source_label(),
                    "treated a cookie credential that failed verification as absent"
                );
                clear_cookie = app.config.credential_source().cookie_name();
                None
            }
            Err(error) => {
                // The one rejection the router never sees: log the credential's place, never its value.
                warn!(
                    error = error.error_code(),
                    source = credential.source_label(),
                    "rejected a credential that failed verification"
                );
                let refusal = answer_navigation(
                    &app,
                    ServerResponse::from_error(&error),
                    method_seen,
                    &headers,
                    &uri,
                    request_id,
                )
                .await;
                // Keyed on a cookie, so private: a cached refusal would refuse the next visitor.
                let rendered = into_axum(if credential.is_cookie() {
                    private_to_the_caller(refusal)
                } else {
                    refusal
                });
                return if is_head {
                    without_body(rendered)
                } else {
                    rendered
                };
            }
        },
        _ => None,
    };

    let locale = negotiated_locale(&app, &headers);

    let preferences = pinned_preferences(&app, &headers);

    drop(parts);
    let request = ServerRequest::new(our_method, uri.path().to_owned(), app.state.clone())
        .with_query(uri.query().unwrap_or_default().to_owned())
        .with_request_id(request_id)
        .with_principal(principal)
        .with_locale(locale)
        .with_preferences(preferences)
        .with_authority(addressed_authority(&headers, &uri).map(str::to_owned))
        .with_headers(neutral);
    let request = match peer {
        Some(peer) => request.with_peer(peer),
        None => request,
    };

    // Read last, after every refusal that needs no body, under the route's own policy.
    let declared_len = headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let (request, deadline, _permit) = match app.router.body_policy(&request) {
        BodyPolicy::Streamed(bounds) => {
            // Held until the response is built, so `max_in_flight` counts requests being served.
            let Some(permit) = bounds.admit() else {
                debug!("refused a streamed body: the route is reading as many as it may");
                return refused(
                    &app,
                    ServerResponse::from_error(&ServerError::StreamBusy)
                        .with_header("Retry-After", "1"),
                    method_seen,
                    &headers,
                    &uri,
                    request_id,
                    is_head,
                )
                .await;
            };
            if declared_len.is_some_and(|len| len > bounds.max_bytes()) {
                let max = bounds.max_bytes();
                return refused(
                    &app,
                    ServerResponse::from_error(&ServerError::PayloadTooLarge { max }),
                    method_seen,
                    &headers,
                    &uri,
                    request_id,
                    is_head,
                )
                .await;
            }
            let guarded = GuardedBody::new(body.into_data_stream(), &bounds);
            (
                request.with_body_stream(RequestBodyStream::new(guarded)),
                bounds.deadline(),
                Some(permit),
            )
        }
        _ => {
            let max = app.config.max_body_size();
            let read = tokio::time::timeout(app.config.request_timeout(), read_buffered(body, max))
                .await
                .unwrap_or(Err(ServerError::RequestTimedOut));
            match read {
                Ok(bytes) => (request.with_body(bytes), app.config.request_timeout(), None),
                Err(error) => {
                    debug!(error = error.error_code(), "request body could not be read");
                    return refused(
                        &app,
                        ServerResponse::from_error(&error),
                        method_seen,
                        &headers,
                        &uri,
                        request_id,
                        is_head,
                    )
                    .await;
                }
            }
        }
    };

    let response = match tokio::time::timeout(deadline, app.router.dispatch(request)).await {
        Ok(response) => response,
        Err(_) => {
            warn!(
                deadline_ms = u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX),
                "a request ran past its deadline and was stopped"
            );
            ServerResponse::from_error(&ServerError::RequestTimedOut)
        }
    };
    let response =
        answer_navigation(&app, response, Some(our_method), &headers, &uri, request_id).await;
    // A language decided by the address needs no language `Vary`: the URL is the whole cache key.
    let response = match app.config.locale_policy() {
        Some(policy) if !response.language_invariant() => varies_on(response, policy.vary()),
        _ => response,
    };
    // A response that depends on a pinned preference varies on its cookie on every representation
    // (RFC 9111 §4.1); a static asset is exempt, being byte-identical to every reader.
    let response = match app.config.preferences().is_empty() || response.preference_invariant() {
        true => response,
        false => app
            .config
            .preferences()
            .iter()
            .fold(response, |response, policy| {
                varies_on(response, policy.vary())
            }),
    };
    // Kept when the handler just set a new cookie of that name, which a clear would race.
    let response = match clear_cookie {
        Some(name) if !sets_cookie(&response, name) => response.with_header(
            "Set-Cookie",
            format!("{name}=; Max-Age=0; Path=/; Secure; HttpOnly; SameSite=Lax"),
        ),
        _ => response,
    };
    // A reply that hands out a credential is as cache-sensitive as one keyed on a session.
    let hands_out_a_credential = response
        .headers()
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("set-cookie"));
    let rendered = into_axum(if from_cookie || hands_out_a_credential {
        private_to_the_caller(response)
    } else {
        response
    });
    if is_head {
        without_body(rendered)
    } else {
        rendered
    }
}

/// Answers a request for an authority this server does not serve with the bare `API_024` envelope.
///
/// Never the error page: a branded page under a look-alike domain is what that domain wants.
async fn refuse_unserved_authority(
    State(config): State<Arc<ServerConfig>>,
    request: Request,
    next: Next,
) -> Response {
    if config.serves(addressed_authority(request.headers(), request.uri())) {
        return next.run(request).await;
    }
    debug!("refused a request addressed to an origin this server does not serve");
    let refused = into_axum(ServerResponse::from_error(&ServerError::MisdirectedRequest));
    if request.method() == HttpMethod::HEAD {
        without_body(refused)
    } else {
        refused
    }
}

/// A refusal made before or around the router, answered as [`answer_navigation`] says.
#[expect(clippy::too_many_arguments, reason = "readability baseline")]
async fn refused<S>(
    app: &AppState<S>,
    response: ServerResponse,
    method: Option<Method>,
    headers: &HeaderMap,
    uri: &Uri,
    request_id: RequestIdResolution,
    is_head: bool,
) -> Response
where
    S: Clone + Send + Sync + 'static,
{
    let rendered =
        into_axum(answer_navigation(app, response, method, headers, uri, request_id).await);
    if is_head {
        without_body(rendered)
    } else {
        rendered
    }
}

/// `response`, answered with the router's error page for a person's navigation.
#[expect(clippy::too_many_arguments, reason = "readability baseline")]
async fn answer_navigation<S>(
    app: &AppState<S>,
    response: ServerResponse,
    method: Option<Method>,
    headers: &HeaderMap,
    uri: &Uri,
    request_id: RequestIdResolution,
) -> ServerResponse
where
    S: Clone + Send + Sync + 'static,
{
    if response.shown_error().is_none() {
        return response;
    }
    let candidate = ServerRequest::new(
        method.unwrap_or(Method::Get),
        uri.path().to_owned(),
        app.state.clone(),
    )
    .with_request_id(request_id)
    .with_locale(negotiated_locale(app, headers))
    .with_preferences(pinned_preferences(app, headers))
    .with_authority(addressed_authority(headers, uri).map(str::to_owned))
    .with_headers(neutral_headers(headers));
    let page_request = app.router.error_page_request(&candidate);
    app.router.answer_navigation(response, page_request).await
}

/// The language this request is answered in; `None` on a surface that negotiates none.
fn negotiated_locale<S>(app: &AppState<S>, headers: &HeaderMap) -> Option<Locale> {
    app.config.locale_policy().map(|policy| {
        policy.negotiate(
            headers
                .get(ACCEPT_LANGUAGE)
                .and_then(|value| value.to_str().ok()),
            cookie_fields(headers),
        )
    })
}

fn pinned_preferences<S>(
    app: &AppState<S>,
    headers: &HeaderMap,
) -> Vec<(&'static str, &'static str)> {
    app.config
        .preferences()
        .iter()
        .filter_map(|policy| {
            policy
                .read(cookie_fields(headers))
                .map(|value| (policy.name(), value))
        })
        .collect()
}

/// Reads a buffered body whole, refusing one past `max` bytes without reading the rest.
async fn read_buffered(body: Body, max: usize) -> Result<Vec<u8>, ServerError> {
    match Limited::new(body, max).collect().await {
        Ok(collected) => Ok(collected.to_bytes().to_vec()),
        Err(error) if error.downcast_ref::<LengthLimitError>().is_some() => {
            Err(ServerError::PayloadTooLarge { max: max as u64 })
        }
        Err(_) => Err(ServerError::BodyInterrupted),
    }
}

fn sets_cookie(response: &ServerResponse, name: &str) -> bool {
    response.headers().iter().any(|(header, value)| {
        header.eq_ignore_ascii_case("set-cookie")
            && value
                .split_once('=')
                .is_some_and(|(cookie, _)| cookie.trim() == name)
    })
}

/// Drops a response's body, keeping every header `GET` would have sent (RFC 9110 §9.3.2).
fn without_body(response: Response) -> Response {
    let (mut parts, body) = response.into_parts();
    let length = match body.size_hint().exact() {
        Some(length) => length,
        // A streamed body of unknown length: `GET` sent no `Content-Length` either.
        None => return Response::from_parts(parts, Body::empty()),
    };
    match HeaderValue::from_str(&length.to_string()) {
        Ok(value) => {
            parts.headers.insert(CONTENT_LENGTH, value);
        }
        Err(error) => {
            tracing::warn!(%error, length, "could not stamp Content-Length on a HEAD response");
        }
    }
    Response::from_parts(parts, Body::empty())
}

/// Declares `response` one of several representations of its URL, keyed on `headers`.
fn varies_on(response: ServerResponse, headers: &[&str]) -> ServerResponse {
    headers.iter().fold(response, |response, header| {
        let already_declared = response.headers().iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("vary")
                && value
                    .split(',')
                    .any(|declared| declared.trim().eq_ignore_ascii_case(header))
        });
        if already_declared {
            response
        } else {
            response.with_header("Vary", *header)
        }
    })
}

/// Marks a response under a cookie credential as private to its caller, with `Vary: Cookie`.
fn private_to_the_caller(response: ServerResponse) -> ServerResponse {
    let handler_set_a_policy = response
        .headers()
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("cache-control"));
    let response = varies_on(response, &["Cookie"]);
    if handler_set_a_policy {
        response
    } else {
        response.with_header("Cache-Control", "private, no-store")
    }
}

/// The axum [`HeaderMap`] as neutral [`RequestHeaders`], skipping a value that is not UTF-8.
fn neutral_headers(headers: &HeaderMap) -> RequestHeaders {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_owned(), value.to_owned()))
        })
        .collect()
}

/// The `host[:port]` the caller addressed: `Host`, or HTTP/2's `:authority` (RFC 9113 §8.3.1).
fn addressed_authority<'a>(headers: &'a HeaderMap, uri: &'a Uri) -> Option<&'a str> {
    headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .or_else(|| uri.authority().map(Authority::as_str))
}

/// The value of every `Cookie` field: HTTP/2 may split the list (RFC 9113 §8.2.3).
fn cookie_fields(headers: &HeaderMap) -> impl Iterator<Item = &str> {
    headers.get_all(COOKIE).iter().filter_map(|value| {
        value.to_str().ok().or_else(|| {
            warn!("skipping a Cookie header field whose value is not valid UTF-8");
            None
        })
    })
}

fn into_axum(mut response: ServerResponse) -> Response {
    let status =
        StatusCode::from_u16(response.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    // An empty content type means no content (a `204`): no header at all.
    let content_type = (!response.content_type().is_empty()).then(|| {
        HeaderValue::from_str(response.content_type())
            .unwrap_or_else(|_| HeaderValue::from_static("application/json"))
    });
    // Read before `into_body` consumes the response; an invalid header is skipped and logged.
    let extra_headers: Vec<(HeaderName, HeaderValue)> = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            match (HeaderName::from_str(name), HeaderValue::from_str(value)) {
                (Ok(name), Ok(value)) => Some((name, value)),
                _ => {
                    tracing::warn!(header = %name, "dropping an invalid response header");
                    None
                }
            }
        })
        .collect();
    let streamed = response.take_stream();
    let mut axum_response = match streamed {
        Some(stream) => {
            let len = stream.exact_len();
            let mut rendered = Response::new(Body::from_stream(stream.into_stream()));
            if let Some(len) = len {
                rendered
                    .headers_mut()
                    .insert(CONTENT_LENGTH, HeaderValue::from(len));
            }
            rendered
        }
        None => Response::new(Body::from(response.into_body())),
    };
    *axum_response.status_mut() = status;
    let response_headers = axum_response.headers_mut();
    if let Some(content_type) = content_type {
        response_headers.insert(CONTENT_TYPE, content_type);
    }
    for (name, value) in extra_headers {
        response_headers.append(name, value);
    }
    axum_response
}
