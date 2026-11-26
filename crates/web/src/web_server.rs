use fundamental_api::ApiServer;
use fundamental_auth::PrincipalVerifier;
use fundamental_core::AnyError;
use fundamental_server::{Router, SecurityHeaders, ServerConfig};
use tracing::error;

use crate::{WebListener, hydration_script_csp_hash};

/// A server-rendered web surface: a [`Router<S>`] of page routes served over HTTP.
pub struct WebServer<S> {
    inner: ApiServer<S>,
}

impl<S> WebServer<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Creates a web server that serves `router` over `state`; no headers set means [`SecurityHeaders::web`].
    pub fn new(config: ServerConfig, state: S, router: Router<S>) -> Self {
        let config = match config.security_headers() {
            Some(_) => config,
            None => config.with_security_headers(Self::default_headers()),
        };
        Self {
            inner: ApiServer::new(config, state, router),
        }
    }

    /// The web default, admitting the hydration script by its hash; strict without it if malformed.
    fn default_headers() -> SecurityHeaders {
        match hydration_script_csp_hash() {
            Ok(hash) => SecurityHeaders::web().with_script_hash(hash),
            Err(problem) => {
                error!(
                    error_code = "API_018",
                    %problem,
                    "the hydration script's hash is malformed; islands will not hydrate"
                );
                SecurityHeaders::web()
            }
        }
    }

    /// Resolves the caller's credential into a principal with `verifier`.
    #[must_use]
    pub fn with_verifier<V>(self, verifier: V) -> Self
    where
        V: PrincipalVerifier + Send + Sync + 'static,
    {
        Self {
            inner: self.inner.with_verifier(verifier),
        }
    }

    /// Binds the configured address and begins serving, returning the running listener.
    ///
    /// # Errors
    /// Returns an [`AnyError`] wrapping
    /// [`ServerError::MissingVerifier`](fundamental_server::ServerError::MissingVerifier)
    /// if principal-gated routes are mounted with no
    /// [`with_verifier`](Self::with_verifier),
    /// [`ServerError::CredentialSourceWithoutVerifier`](fundamental_server::ServerError::CredentialSourceWithoutVerifier)
    /// if a cookie credential source is configured with no verifier, or
    /// [`ServerError::Bind`](fundamental_server::ServerError::Bind) if the
    /// configured address cannot be bound.
    pub async fn serve(self) -> Result<WebListener, AnyError> {
        self.inner.serve().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_server::{HttpContext, Route, ServerListener};
    use leptos::prelude::*;

    use crate::render_to_html;

    #[derive(Clone)]
    struct State {
        title: &'static str,
    }

    fn router() -> Router<State> {
        Router::new()
            .route(Route::html("/", |ctx: HttpContext<State>| async move {
                let title = ctx.state().title;
                Ok(render_to_html(move || {
                    view! { <main><h1>{title}</h1></main> }
                }))
            }))
            .expect("unique route")
    }

    #[tokio::test]
    async fn serves_a_server_rendered_page() {
        let config = ServerConfig::new("127.0.0.1:0".parse().expect("addr"));
        let listener = WebServer::new(config, State { title: "Home" }, router())
            .serve()
            .await
            .expect("binds an ephemeral port");
        let url = format!("http://{}/", listener.local_addr());

        let response = reqwest::Client::new()
            .get(&url)
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 200);
        assert_eq!(
            response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok()),
            Some("text/html; charset=utf-8"),
        );
        let body = response.text().await.expect("text body");
        assert!(body.contains("<h1>"), "body: {body}");
        assert!(body.contains("Home"), "body: {body}");

        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn a_default_web_server_is_strict() {
        let config = ServerConfig::new("127.0.0.1:0".parse().expect("addr"));
        let listener = WebServer::new(config, State { title: "Home" }, router())
            .serve()
            .await
            .expect("binds an ephemeral port");
        let response = reqwest::Client::new()
            .get(format!("http://{}/", listener.local_addr()))
            .send()
            .await
            .expect("request sent");
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        let hash = crate::hydration_script_csp_hash()
            .expect("well formed")
            .source();
        let csp = header("content-security-policy").expect("a csp");
        assert!(
            csp.contains(&format!("script-src 'self' 'wasm-unsafe-eval' {hash}")),
            "{csp}"
        );
        assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
        assert_eq!(header("x-frame-options").as_deref(), Some("DENY"));
        assert_eq!(header("x-content-type-options").as_deref(), Some("nosniff"));
        assert_eq!(
            header("cross-origin-opener-policy").as_deref(),
            Some("same-origin")
        );
        assert_eq!(
            header("strict-transport-security"),
            None,
            "HSTS is configured, not assumed"
        );
        listener.stop().await.expect("stops cleanly");
    }

    use fundamental_auth::{AuthError, AuthenticatedPrincipal, InvalidToken, UserId};
    use fundamental_core::secrets::SecretString;
    use fundamental_server::CredentialSource;

    const SESSION: &str = "__Host-session";

    struct StubVerifier;

    impl PrincipalVerifier for StubVerifier {
        async fn verify(&self, token: &SecretString) -> Result<AuthenticatedPrincipal, AuthError> {
            if token.expose() == "good-token" {
                Ok(AuthenticatedPrincipal {
                    user_id: UserId::new("staff-1"),
                    device_id: None,
                    scopes: Vec::new(),
                    expires_at: 0,
                })
            } else {
                Err(AuthError::Invalid(InvalidToken::Unrecognised))
            }
        }
    }

    fn signed_in_router() -> Router<State> {
        router()
            .route(
                Route::post("/save", |ctx: HttpContext<State>, _: ()| async move {
                    let principal = ctx.principal()?;
                    Ok(format!("saved for {}", principal.user_id))
                })
                .middleware(fundamental_server::auth),
            )
            .expect("unique route")
    }

    async fn start_signed_in() -> WebListener {
        WebServer::new(
            ServerConfig::new("127.0.0.1:0".parse().expect("addr"))
                .with_credential_source(CredentialSource::cookie(SESSION)),
            State { title: "Home" },
            signed_in_router(),
        )
        .with_verifier(StubVerifier)
        .serve()
        .await
        .expect("binds an ephemeral port")
    }

    #[tokio::test]
    async fn a_web_surface_can_serve_an_auth_gated_route() {
        let listener = start_signed_in().await;
        assert!(
            listener
                .local_addr()
                .address()
                .expect("a bound address")
                .port()
                > 0
        );
        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn a_session_cookie_authenticates_a_write() {
        let listener = start_signed_in().await;
        let addr = listener.local_addr();

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/save"))
            .header("cookie", format!("{SESSION}=good-token"))
            .header("origin", format!("http://{addr}"))
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 200);
        assert!(
            response
                .text()
                .await
                .expect("text body")
                .contains("staff-1"),
            "the page's own action ran with the principal the cookie resolved to"
        );

        listener.stop().await.expect("stops cleanly");
    }

    // A cross-site `<form>` needs no preflight, and a body is decoded without consulting `Content-Type`.
    #[tokio::test]
    async fn a_cross_site_form_post_cannot_use_the_session() {
        let listener = start_signed_in().await;
        let addr = listener.local_addr();

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/save"))
            .header("cookie", format!("{SESSION}=good-token"))
            .header("origin", "https://evil.test")
            .header("content-type", "application/x-www-form-urlencoded")
            .body("")
            .send()
            .await
            .expect("request sent");
        assert_eq!(response.status(), 403);
        let value: serde_json::Value = response.json().await.expect("json body");
        assert_eq!(value["error"], "API_015");

        listener.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn a_page_is_still_served_to_a_signed_out_visitor() {
        let listener = start_signed_in().await;
        let response = reqwest::Client::new()
            .get(format!("http://{}/", listener.local_addr()))
            .send()
            .await
            .expect("request sent");
        assert_eq!(
            response.status(),
            200,
            "a public page must not require the session an action does"
        );

        listener.stop().await.expect("stops cleanly");
    }
}
