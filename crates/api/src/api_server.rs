use std::sync::Arc;

use fundamental_auth::PrincipalVerifier;
use fundamental_core::{AnyError, Error};
use fundamental_server::{Router, Server, ServerConfig, ServerError};
use tracing::{error, info, warn};

use crate::api_listener::ApiListener;
use crate::axum_backend::AxumBackend;
use crate::boxed_verifier::BoxedVerifier;

/// An HTTP API server backed by axum.
///
/// ```no_run
/// # extern crate fundamental_api;
/// # async fn run() -> Result<(), fundamental_core::AnyError> {
/// use fundamental_api::{ApiServer, HttpContext, Route, Router, ServerConfig};
///
/// #[derive(Clone)]
/// struct AppState;
///
/// let router = Router::<AppState>::new()
///     .route(Route::get("/health", |_ctx: HttpContext<AppState>, _: ()| async {
///         Ok("ok")
///     }))?;
///
/// let config = ServerConfig::new("127.0.0.1:16001".parse().expect("addr"));
/// let listener = ApiServer::new(config, AppState, router).serve().await?;
/// # let _ = listener;
/// # Ok(())
/// # }
/// ```
pub struct ApiServer<S> {
    config: ServerConfig,
    state: S,
    router: Router<S>,
    verifier: Option<Arc<dyn BoxedVerifier>>,
    require_object_authorization: bool,
}

impl<S> ApiServer<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Creates a server that serves `router` over `state`, bound per `config`.
    pub fn new(config: ServerConfig, state: S, router: Router<S>) -> Self {
        Self {
            config,
            state,
            router,
            verifier: None,
            require_object_authorization: false,
        }
    }

    /// Resolves bearer tokens into principals with `verifier`.
    #[must_use]
    pub fn with_verifier<V>(mut self, verifier: V) -> Self
    where
        V: PrincipalVerifier + Send + Sync + 'static,
    {
        self.verifier = Some(Arc::new(verifier) as Arc<dyn BoxedVerifier>);
        self
    }

    /// Refuses to start if any mounted write reachable by an identified caller authorizes no object.
    ///
    /// Opt-in, because only an app whose objects are modelled in a relation schema can satisfy it.
    /// A route whose act names no single object says so with
    /// [`Route::no_single_object`](fundamental_server::Route::no_single_object).
    #[must_use]
    pub fn require_object_authorization(mut self) -> Self {
        self.require_object_authorization = true;
        self
    }

    /// Binds the configured address and begins serving, returning the running listener.
    ///
    /// # Errors
    /// Returns an [`AnyError`] wrapping
    /// [`ServerError::MissingVerifier`](fundamental_server::ServerError::MissingVerifier)
    /// if principal-gated routes are mounted with no verifier,
    /// [`ServerError::CredentialSourceWithoutVerifier`](fundamental_server::ServerError::CredentialSourceWithoutVerifier)
    /// if a cookie credential source is configured with no verifier,
    /// [`ServerError::UnauthorizedWrites`](fundamental_server::ServerError::UnauthorizedWrites)
    /// if [`require_object_authorization`](Self::require_object_authorization) was
    /// asked for and a mounted write authorizes no object,
    /// [`ServerError::SegmentsMisdeclared`](fundamental_server::ServerError::SegmentsMisdeclared)
    /// if [`Router::segment_problems`](fundamental_server::Router::segment_problems) names any,
    /// [`ServerError::StreamedRouteReadsBody`](fundamental_server::ServerError::StreamedRouteReadsBody)
    /// if a layer that reads the body guards a streamed route, or
    /// [`ServerError::Bind`](fundamental_server::ServerError::Bind) if the
    /// configured address cannot be bound.
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "readability baseline"
    )]
    pub async fn serve(self) -> Result<ApiListener, AnyError> {
        if self.verifier.is_none() {
            let gated = self.router.principal_gated_routes();
            if !gated.is_empty() {
                let routes = gated
                    .iter()
                    .map(|(method, path)| format!("{method} {path}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let error = ServerError::MissingVerifier { routes };
                error!(
                    error = error.error_code(),
                    count = gated.len(),
                    "refusing to serve: principal-gated routes with no verifier"
                );
                return Err(AnyError::new(error));
            }

            if let Some(cookie) = self.config.credential_source().cookie_name() {
                let error = ServerError::CredentialSourceWithoutVerifier {
                    cookie: cookie.to_owned(),
                };
                error!(
                    error = error.error_code(),
                    "refusing to serve: a cookie credential source with no verifier"
                );
                return Err(AnyError::new(error));
            }
        }

        if self.require_object_authorization {
            let unguarded = self.router.authenticated_unauthorized_writes();
            if !unguarded.is_empty() {
                let routes = unguarded
                    .iter()
                    .map(|(method, path)| format!("{method} {path}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let error = ServerError::UnauthorizedWrites { routes };
                error!(
                    error = error.error_code(),
                    count = unguarded.len(),
                    "refusing to serve: authenticated writes that authorize no object"
                );
                return Err(AnyError::new(error));
            }
        }

        let mut cross_origin_problems: Vec<String> = self
            .router
            .cross_origin_routes()
            .into_iter()
            .filter(|(_, _, set)| {
                self.config
                    .cross_origins(set)
                    .is_none_or(<[String]>::is_empty)
            })
            .map(|(method, path, set)| format!("{method} {path} names the empty set \"{set}\""))
            .collect();
        cross_origin_problems.extend(
            self.router
                .cross_origin_routes_needing_a_principal()
                .into_iter()
                .map(|(method, path)| {
                    format!("{method} {path} reads no credential yet needs a principal")
                }),
        );
        if !cross_origin_problems.is_empty() {
            let error = ServerError::CrossOriginMisdeclared {
                problems: cross_origin_problems.join("; "),
            };
            error!(
                error = error.error_code(),
                count = cross_origin_problems.len(),
                "refusing to serve: cross-origin routes that can answer no one"
            );
            return Err(AnyError::new(error));
        }

        let misdeclared = self.router.segment_problems();
        if !misdeclared.is_empty() {
            let error = ServerError::SegmentsMisdeclared {
                problems: misdeclared.join("; "),
            };
            error!(
                error = error.error_code(),
                count = misdeclared.len(),
                "refusing to serve: path segments are declared inconsistently"
            );
            return Err(AnyError::new(error));
        }

        let reading = self.router.streamed_routes_reading_body();
        if !reading.is_empty() {
            let routes = reading
                .iter()
                .map(|(method, path)| format!("{method} {path}"))
                .collect::<Vec<_>>()
                .join(", ");
            let error = ServerError::StreamedRouteReadsBody { routes };
            error!(
                error = error.error_code(),
                count = reading.len(),
                "refusing to serve: body-reading middleware in front of streamed routes"
            );
            return Err(AnyError::new(error));
        }

        let source = self.config.credential_source();
        if source.reads_cookie() && !source.has_prefixed_cookie_name() {
            // Without `__Host-` a sibling subdomain can set a shadow cookie that sorts first (RFC 6265 §5.4).
            warn!(
                cookie = source.cookie_name().unwrap_or_default(),
                "credential cookie is not `__Host-`/`__Secure-` prefixed: a sibling subdomain can shadow it"
            );
        }
        info!(credential_source = ?source, "resolving principals from this credential source");

        let backend = AxumBackend::with_verifier(self.verifier);
        Server::new(backend, self.config, self.state, self.router)
            .serve()
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_server::{HttpContext, Route, ServerListener};
    use serde::{Deserialize, Serialize};

    #[derive(Clone)]
    struct State {
        greeting: &'static str,
    }

    #[derive(Deserialize)]
    struct Name {
        name: String,
    }

    #[derive(Serialize, Deserialize)]
    struct Greeting {
        text: String,
    }

    fn router() -> Router<State> {
        Router::new()
            .route(Route::post(
                "/greet",
                |ctx: HttpContext<State>, input: Name| async move {
                    Ok(Greeting {
                        text: format!("{}, {}", ctx.state().greeting, input.name),
                    })
                },
            ))
            .expect("unique route")
    }

    fn config() -> ServerConfig {
        ServerConfig::new("127.0.0.1:0".parse().expect("addr"))
    }

    async fn start(config: ServerConfig) -> ApiListener {
        ApiServer::new(config, State { greeting: "Hello" }, router())
            .serve()
            .await
            .expect("binds an ephemeral port")
    }

    #[tokio::test]
    async fn round_trips_a_request() {
        let listener = start(config()).await;
        let url = format!("http://{}/greet", listener.local_addr());

        let response = reqwest::Client::new()
            .post(&url)
            .json(&serde_json::json!({ "name": "Ada" }))
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 200);
        let greeting: Greeting = response.json().await.expect("json body");
        assert_eq!(greeting.text, "Hello, Ada");

        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn unknown_path_is_404() {
        let listener = start(config()).await;
        let url = format!("http://{}/missing", listener.local_addr());

        let response = reqwest::Client::new()
            .get(&url)
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 404);
        let value: serde_json::Value = response.json().await.expect("json body");
        assert_eq!(value["error"], "API_002");

        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn wrong_method_is_405() {
        let listener = start(config()).await;
        let url = format!("http://{}/greet", listener.local_addr());

        let response = reqwest::Client::new()
            .delete(&url)
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 405);

        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn oversized_body_is_rejected() {
        let listener = start(config().with_max_body_size(8)).await;
        let url = format!("http://{}/greet", listener.local_addr());

        let response = reqwest::Client::new()
            .post(&url)
            .body("x".repeat(64))
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 413);

        listener.stop().await.expect("stops cleanly");
    }

    use std::future::Future;

    use fundamental_auth::{
        AuthError, AuthenticatedPrincipal, InvalidToken, PrincipalVerifier, UserId,
    };
    use fundamental_core::secrets::SecretString;

    struct StubVerifier;

    impl PrincipalVerifier for StubVerifier {
        #[expect(clippy::excessive_nesting, reason = "readability baseline")]
        fn verify(
            &self,
            token: &SecretString,
        ) -> impl Future<Output = Result<AuthenticatedPrincipal, AuthError>> + Send {
            let valid = token.expose() == "good-token";
            async move {
                if valid {
                    Ok(AuthenticatedPrincipal {
                        user_id: UserId::new("u1"),
                        device_id: None,
                        scopes: Vec::new(),
                        expires_at: 0,
                    })
                } else {
                    Err(AuthError::Invalid(InvalidToken::Unrecognised))
                }
            }
        }
    }

    fn me_router() -> Router<State> {
        Router::new()
            .route(
                Route::get("/me", |ctx: HttpContext<State>, _: ()| async move {
                    let principal = ctx.principal()?;
                    Ok(Greeting {
                        text: format!("you are {}", principal.user_id),
                    })
                })
                .middleware(fundamental_server::auth),
            )
            .expect("unique route")
    }

    async fn start_authenticated() -> ApiListener {
        ApiServer::new(config(), State { greeting: "Hello" }, me_router())
            .with_verifier(StubVerifier)
            .serve()
            .await
            .expect("binds an ephemeral port")
    }

    #[tokio::test]
    async fn refuses_to_serve_gated_routes_with_no_verifier() {
        let Err(error) = ApiServer::new(config(), State { greeting: "Hello" }, me_router())
            .serve()
            .await
        else {
            panic!("an unsatisfiable route must not be served");
        };

        assert_eq!(error.get().error_code(), "API_013");
        assert_eq!(error.get().http_status_code(), 500);
        assert!(
            error.to_string().contains("GET /me"),
            "the operator must be told which routes are unsatisfiable: {error}"
        );
    }

    #[tokio::test]
    async fn refuses_to_serve_an_authenticated_write_that_authorizes_no_object() {
        let router = Router::<State>::new()
            .route(
                Route::post(
                    "/credentials/{id}/revoke",
                    |_ctx: HttpContext<State>, _: ()| async move { Ok(()) },
                )
                .middleware(fundamental_server::auth),
            )
            .expect("unique route");

        let Err(error) = ApiServer::new(config(), State { greeting: "Hello" }, router)
            .with_verifier(StubVerifier)
            .require_object_authorization()
            .serve()
            .await
        else {
            panic!("an unguarded authenticated write must not be served");
        };

        assert_eq!(error.get().error_code(), "API_017");
        assert_eq!(error.get().http_status_code(), 500);
        assert!(
            error.to_string().contains("POST /credentials/{id}/revoke"),
            "the operator must be told which writes are unguarded: {error}"
        );
    }

    #[tokio::test]
    async fn an_app_that_did_not_ask_serves_the_same_route() {
        let router = Router::<State>::new()
            .route(
                Route::post(
                    "/credentials/{id}/revoke",
                    |_ctx: HttpContext<State>, _: ()| async move { Ok(()) },
                )
                .middleware(fundamental_server::auth),
            )
            .expect("unique route");

        let listener = ApiServer::new(config(), State { greeting: "Hello" }, router)
            .with_verifier(StubVerifier)
            .serve()
            .await
            .expect("an app that did not ask must still serve");
        drop(listener);
    }

    #[tokio::test]
    async fn a_write_declaring_no_single_object_still_serves() {
        let router = Router::<State>::new()
            .route(
                Route::post(
                    "/erase",
                    |_ctx: HttpContext<State>, _: ()| async move { Ok(()) },
                )
                .middleware(fundamental_server::auth)
                .no_single_object(),
            )
            .expect("unique route");

        let listener = ApiServer::new(config(), State { greeting: "Hello" }, router)
            .with_verifier(StubVerifier)
            .require_object_authorization()
            .serve()
            .await
            .expect("a declared act with no single object must serve");
        drop(listener);
    }

    #[tokio::test]
    async fn refusing_an_unguarded_write_leaves_the_port_unbound() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve a port");
        let addr = listener.local_addr().expect("addr");
        drop(listener);

        let router = Router::<State>::new()
            .route(
                Route::post("/revoke", |_ctx: HttpContext<State>, _: ()| async move {
                    Ok(())
                })
                .middleware(fundamental_server::auth),
            )
            .expect("unique route");

        let Err(error) =
            ApiServer::new(ServerConfig::new(addr), State { greeting: "Hello" }, router)
                .with_verifier(StubVerifier)
                .require_object_authorization()
                .serve()
                .await
        else {
            panic!("an unguarded authenticated write must not be served");
        };
        assert_eq!(error.get().error_code(), "API_017");

        std::net::TcpListener::bind(addr)
            .expect("the address must be free: refusing happens before the bind");
    }

    // Refused before binding, or a supervisor sees a port open on a process about to die.
    #[tokio::test]
    async fn refusing_leaves_the_port_unbound() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve a port");
        let addr = listener.local_addr().expect("addr");
        drop(listener);

        let Err(error) = ApiServer::new(
            ServerConfig::new(addr),
            State { greeting: "Hello" },
            me_router(),
        )
        .serve()
        .await
        else {
            panic!("gated routes with no verifier must not be served");
        };
        assert_eq!(error.get().error_code(), "API_013");

        std::net::TcpListener::bind(addr).expect("the refused server bound nothing");
    }

    #[tokio::test]
    async fn an_ungated_router_still_serves_without_a_verifier() {
        let listener = start(config()).await;
        assert!(
            listener
                .local_addr()
                .address()
                .is_some_and(|bound| bound.port() > 0)
        );
    }

    #[tokio::test]
    async fn auth_route_without_a_token_is_401() {
        let listener = start_authenticated().await;
        let url = format!("http://{}/me", listener.local_addr());

        let response = reqwest::Client::new()
            .get(&url)
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 401);
        let value: serde_json::Value = response.json().await.expect("json body");
        assert_eq!(value["error"], "API_009");

        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn auth_route_with_a_bad_token_is_401() {
        let listener = start_authenticated().await;
        let url = format!("http://{}/me", listener.local_addr());

        let response = reqwest::Client::new()
            .get(&url)
            .bearer_auth("nope")
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 401);
        let value: serde_json::Value = response.json().await.expect("json body");
        assert_eq!(value["error"], "AUTH_002");

        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn auth_route_with_a_good_token_runs_with_the_principal() {
        let listener = start_authenticated().await;
        let url = format!("http://{}/me", listener.local_addr());

        let response = reqwest::Client::new()
            .get(&url)
            .bearer_auth("good-token")
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 200);
        let greeting: Greeting = response.json().await.expect("json body");
        assert_eq!(greeting.text, "you are u1");

        listener.stop().await.expect("stops cleanly");
    }

    use fundamental_server::CredentialSource;

    const SESSION: &str = "__Host-session";

    fn session_router() -> Router<State> {
        me_router()
            .route(
                Route::post("/write", |ctx: HttpContext<State>, _: ()| async move {
                    let principal = ctx.principal()?;
                    Ok(Greeting {
                        text: format!("{} wrote", principal.user_id),
                    })
                })
                .middleware(fundamental_server::auth),
            )
            .expect("unique route")
    }

    async fn start_with_source(source: CredentialSource) -> ApiListener {
        ApiServer::new(
            config().with_credential_source(source),
            State { greeting: "Hello" },
            session_router(),
        )
        .with_verifier(StubVerifier)
        .serve()
        .await
        .expect("binds an ephemeral port")
    }

    #[tokio::test]
    async fn a_good_session_cookie_authenticates() {
        let listener = start_with_source(CredentialSource::cookie(SESSION)).await;
        let url = format!("http://{}/me", listener.local_addr());

        let response = reqwest::Client::new()
            .get(&url)
            .header("cookie", format!("{SESSION}=good-token"))
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 200);
        let greeting: Greeting = response.json().await.expect("json body");
        assert_eq!(greeting.text, "you are u1");

        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn a_bad_session_cookie_reaches_the_auth_gate_as_no_credential() {
        let listener = start_with_source(CredentialSource::cookie(SESSION)).await;
        let url = format!("http://{}/me", listener.local_addr());

        let response = reqwest::Client::new()
            .get(&url)
            .header("cookie", format!("{SESSION}=nope"))
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 401);
        assert!(
            response
                .headers()
                .get_all("set-cookie")
                .iter()
                .filter_map(|value| value.to_str().ok())
                .any(|value| value.starts_with(&format!("{SESSION}=;"))),
            "the dead cookie is cleared"
        );
        let value: serde_json::Value = response.json().await.expect("json body");
        assert_eq!(value["error"], "API_009");

        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn no_cookie_is_the_auth_gates_401() {
        let listener = start_with_source(CredentialSource::cookie(SESSION)).await;
        let url = format!("http://{}/me", listener.local_addr());

        let response = reqwest::Client::new()
            .get(&url)
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 401);
        let value: serde_json::Value = response.json().await.expect("json body");
        assert_eq!(value["error"], "API_009");

        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn a_bearer_does_not_authenticate_a_cookie_only_surface() {
        let listener = start_with_source(CredentialSource::cookie(SESSION)).await;
        let url = format!("http://{}/me", listener.local_addr());

        let response = reqwest::Client::new()
            .get(&url)
            .bearer_auth("good-token")
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 401);
        let value: serde_json::Value = response.json().await.expect("json body");
        assert_eq!(value["error"], "API_009");

        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn bearer_then_cookie_admits_both_kinds_of_caller() {
        let listener = start_with_source(CredentialSource::bearer_then_cookie(SESSION)).await;
        let url = format!("http://{}/me", listener.local_addr());
        let client = reqwest::Client::new();

        for request in [
            client.get(&url).bearer_auth("good-token"),
            client
                .get(&url)
                .header("cookie", format!("{SESSION}=good-token")),
        ] {
            let response = request.send().await.expect("request sent");
            assert_eq!(response.status(), 200);
        }

        listener.stop().await.expect("stops cleanly");
    }

    // A presented credential that failed is the answer; falling through would let a caller shop.
    #[tokio::test]
    async fn a_failing_bearer_is_not_retried_against_the_cookie() {
        let listener = start_with_source(CredentialSource::bearer_then_cookie(SESSION)).await;
        let url = format!("http://{}/me", listener.local_addr());

        let response = reqwest::Client::new()
            .get(&url)
            .bearer_auth("nope")
            .header("cookie", format!("{SESSION}=good-token"))
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 401);
        let value: serde_json::Value = response.json().await.expect("json body");
        assert_eq!(value["error"], "AUTH_002");

        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn a_cross_site_cookie_write_is_refused() {
        let listener = start_with_source(CredentialSource::cookie(SESSION)).await;
        let url = format!("http://{}/write", listener.local_addr());

        let response = reqwest::Client::new()
            .post(&url)
            .header("cookie", format!("{SESSION}=good-token"))
            .header("origin", "https://evil.test")
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 403);
        let value: serde_json::Value = response.json().await.expect("json body");
        assert_eq!(value["error"], "API_015");

        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn a_same_origin_cookie_write_succeeds() {
        let listener = start_with_source(CredentialSource::cookie(SESSION)).await;
        let addr = listener.local_addr().address().expect("a bound address");
        let url = format!("http://{addr}/write");

        let response = reqwest::Client::new()
            .post(&url)
            .header("cookie", format!("{SESSION}=good-token"))
            .header("origin", format!("http://{addr}"))
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 200);
        let greeting: Greeting = response.json().await.expect("json body");
        assert_eq!(greeting.text, "u1 wrote");

        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn a_cookie_write_with_no_origin_signal_is_refused() {
        let listener = start_with_source(CredentialSource::cookie(SESSION)).await;
        let url = format!("http://{}/write", listener.local_addr());

        let response = reqwest::Client::new()
            .post(&url)
            .header("cookie", format!("{SESSION}=good-token"))
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 403);

        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn a_bearer_write_is_never_guarded() {
        let listener = start_with_source(CredentialSource::bearer_then_cookie(SESSION)).await;
        let url = format!("http://{}/write", listener.local_addr());

        let response = reqwest::Client::new()
            .post(&url)
            .bearer_auth("good-token")
            .header("origin", "https://any.test")
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 200);

        listener.stop().await.expect("stops cleanly");
    }

    // RFC 9111 §3.5 covers `Authorization` but not `Cookie`, so a CDN would share the page.
    #[tokio::test]
    async fn a_cookie_authenticated_response_is_private_and_varies_on_cookie() {
        let listener = start_with_source(CredentialSource::cookie(SESSION)).await;
        let url = format!("http://{}/me", listener.local_addr());

        let response = reqwest::Client::new()
            .get(&url)
            .header("cookie", format!("{SESSION}=good-token"))
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 200);
        // The CORS layer emits its own `Vary`, so ours may arrive as a second field line.
        assert!(
            values(&response, "vary").iter().any(|v| v == "Cookie"),
            "{:?}",
            values(&response, "vary")
        );
        assert_eq!(values(&response, "cache-control"), ["private, no-store"]);

        listener.stop().await.expect("stops cleanly");
    }

    fn values(response: &reqwest::Response, name: &str) -> Vec<String> {
        response
            .headers()
            .get_all(name)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .map(str::to_owned)
            .collect()
    }

    #[tokio::test]
    async fn a_bearer_authenticated_response_is_left_alone() {
        let listener = start_authenticated().await;
        let url = format!("http://{}/me", listener.local_addr());

        let response = reqwest::Client::new()
            .get(&url)
            .bearer_auth("good-token")
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 200);
        assert!(
            !values(&response, "vary").iter().any(|v| v == "Cookie"),
            "a bearer response must not claim to vary on a cookie it never read"
        );
        assert!(
            values(&response, "cache-control").is_empty(),
            "the bearer path's cacheability is unchanged — RFC 9111 §3.5 already covers it"
        );

        listener.stop().await.expect("stops cleanly");
    }

    // A shadow set on a sibling subdomain sorts ahead of the real session (RFC 6265 §5.4).
    #[tokio::test]
    async fn an_ambiguous_session_cookie_is_refused() {
        let listener = start_with_source(CredentialSource::cookie(SESSION)).await;
        let url = format!("http://{}/me", listener.local_addr());

        let response = reqwest::Client::new()
            .get(&url)
            .header(
                "cookie",
                format!("{SESSION}=attacker; {SESSION}=good-token"),
            )
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 401);
        let value: serde_json::Value = response.json().await.expect("json body");
        assert_eq!(value["error"], "AUTH_009");

        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn refuses_to_serve_a_cookie_source_with_no_verifier() {
        let Err(error) = ApiServer::new(
            config().with_credential_source(CredentialSource::cookie(SESSION)),
            State { greeting: "Hello" },
            router(),
        )
        .serve()
        .await
        else {
            panic!("a cookie source with no verifier must not be served");
        };

        assert_eq!(error.get().error_code(), "API_014");
        assert_eq!(error.get().http_status_code(), 500);
        assert!(
            error.to_string().contains(SESSION),
            "the operator must be told which cookie is unread: {error}"
        );
    }

    // Not conditioned on gated routes: a cookie read into nothing is a silent no-op either way.
    #[tokio::test]
    async fn refusing_a_cookie_source_leaves_the_port_unbound() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve a port");
        let addr = listener.local_addr().expect("addr");
        drop(listener);

        let Err(error) = ApiServer::new(
            ServerConfig::new(addr).with_credential_source(CredentialSource::cookie(SESSION)),
            State { greeting: "Hello" },
            router(),
        )
        .serve()
        .await
        else {
            panic!("a cookie source with no verifier must not be served");
        };
        assert_eq!(error.get().error_code(), "API_014");

        std::net::TcpListener::bind(addr).expect("the refused server bound nothing");
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn a_rejected_credential_is_logged_by_place_and_never_by_value() {
        let listener = start_with_source(CredentialSource::cookie(SESSION)).await;
        let url = format!("http://{}/me", listener.local_addr());

        let response = reqwest::Client::new()
            .get(&url)
            .header("cookie", format!("{SESSION}=super-secret-session"))
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 401);

        assert!(logs_contain(
            "treated a cookie credential that failed verification as absent"
        ));
        assert!(logs_contain("cookie"));
        assert!(
            !logs_contain("super-secret-session"),
            "the credential must never reach a log, and `SetSensitiveRequestHeadersLayer` \
             must keep the `Cookie` header out of the request trace"
        );

        listener.stop().await.expect("stops cleanly");
    }

    // `allow_dev_tools_cors` admits any loopback origin, so credentials must never be granted.
    #[tokio::test]
    async fn the_cors_layer_never_grants_credentials() {
        let listener = start_with_source(CredentialSource::cookie(SESSION)).await;
        let url = format!("http://{}/me", listener.local_addr());

        let response = reqwest::Client::new()
            .get(&url)
            .header("origin", "https://other.test")
            .header("cookie", format!("{SESSION}=good-token"))
            .send()
            .await
            .expect("request sent");
        assert!(
            response
                .headers()
                .get("access-control-allow-credentials")
                .is_none(),
            "a cross-origin caller must never be told it may send this surface credentials"
        );

        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn gated_routes_with_no_verifier_still_report_the_route_error() {
        let Err(error) = ApiServer::new(
            config().with_credential_source(CredentialSource::cookie(SESSION)),
            State { greeting: "Hello" },
            me_router(),
        )
        .serve()
        .await
        else {
            panic!("an unsatisfiable route must not be served");
        };
        assert_eq!(error.get().error_code(), "API_013");
    }
}
