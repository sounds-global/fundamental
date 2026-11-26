use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use fundamental_server::{RequestIdResolution, Router, ServerRequest};
use tracing::{Instrument, debug, info_span};

use crate::{ActionBridge, Anonymous, BridgeRequest, BridgeResponse, NativeError, PrincipalSource};

/// The default [`ActionBridge`]: run every action inside the app process.
///
/// Dispatches on the [`Router`] directly with no port, which is why it is the mobile-safe
/// default, and resolves the caller through its [`PrincipalSource`].
pub struct InProcessBridge<S> {
    router: Arc<Router<S>>,
    state: S,
    principal: Arc<dyn PrincipalSource>,
}

impl<S> InProcessBridge<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Creates a bridge over `router` and `state`, dispatching anonymously.
    pub fn new(router: Router<S>, state: S) -> Self {
        Self::shared(Arc::new(router), state)
    }

    /// Creates a bridge over an already-shared router.
    pub fn shared(router: Arc<Router<S>>, state: S) -> Self {
        Self {
            router,
            state,
            principal: Arc::new(Anonymous),
        }
    }

    /// Replaces the principal source resolved before each dispatch.
    #[must_use]
    pub fn with_principal(mut self, source: impl PrincipalSource) -> Self {
        self.principal = Arc::new(source);
        self
    }
}

// Manual, because the router and principal source are not `Debug`.
impl<S: std::fmt::Debug> std::fmt::Debug for InProcessBridge<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InProcessBridge")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl<S> ActionBridge for InProcessBridge<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn call(
        &self,
        request: BridgeRequest,
    ) -> Pin<Box<dyn Future<Output = Result<BridgeResponse, NativeError>> + Send + '_>> {
        // Minted here so this span and the router's carry one id; no path, which is the webview's.
        let request_id = RequestIdResolution::minted();
        let span = info_span!(
            "native.invoke",
            bridge = "in_process",
            method = %request.method,
            request_id = %request_id.id(),
            status = tracing::field::Empty,
        );

        Box::pin(
            async move {
                let principal = self
                    .principal
                    .resolve()
                    .await
                    .map_err(|source| NativeError::Principal { source })?;
                debug!(
                    authenticated = principal.is_some(),
                    "principal resolved, dispatching in-process"
                );

                // The query and headers ride across as the API backend sends them.
                let server_request =
                    ServerRequest::new(request.method, request.path, self.state.clone())
                        .with_query(request.query)
                        .with_headers(request.headers)
                        .with_body(request.body)
                        .with_request_id(request_id)
                        .with_principal(principal);
                let response: BridgeResponse = self
                    .router
                    .dispatch(server_request)
                    .await
                    .buffered()
                    .await
                    .into();
                tracing::Span::current().record("status", response.status);
                Ok(response)
            }
            .instrument(span),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::InProcessBridge;
    use crate::{ActionBridge, BridgeRequest, NativeError, PrincipalSource};
    use fundamental_auth::AuthenticatedPrincipal;
    use fundamental_core::AnyError;
    use fundamental_server::{HttpContext, Method, RequestHeaders, Route, Router};
    use std::future::Future;
    use std::pin::Pin;

    fn test_router() -> Router<()> {
        Router::new()
            .route(Route::post(
                "/api/echo",
                |_ctx: HttpContext<()>, input: serde_json::Value| async move {
                    Ok::<_, AnyError>(input)
                },
            ))
            .expect("unique route")
            .route(Route::get(
                "/api/whoami",
                |ctx: HttpContext<()>, _input: ()| async move {
                    let who = ctx
                        .principal_opt()
                        .map(|p| p.user_id.to_string())
                        .unwrap_or_else(|| "anonymous".to_string());
                    Ok::<_, AnyError>(who)
                },
            ))
            .expect("unique route")
            .route(Route::get(
                "/api/metadata",
                |ctx: HttpContext<()>, _input: ()| async move {
                    Ok::<_, AnyError>(format!(
                        "{}|{}",
                        ctx.header("sec-fetch-mode").unwrap_or("<absent>"),
                        ctx.query_raw()
                    ))
                },
            ))
            .expect("unique route")
    }

    struct FixedPrincipal;
    impl PrincipalSource for FixedPrincipal {
        fn resolve(
            &self,
        ) -> Pin<
            Box<dyn Future<Output = Result<Option<AuthenticatedPrincipal>, AnyError>> + Send + '_>,
        > {
            Box::pin(async {
                Ok(Some(AuthenticatedPrincipal {
                    user_id: fundamental_auth::UserId::new("user-7"),
                    device_id: None,
                    scopes: vec![],
                    expires_at: i64::MAX,
                }))
            })
        }
    }

    struct FailingPrincipal;
    impl PrincipalSource for FailingPrincipal {
        fn resolve(
            &self,
        ) -> Pin<
            Box<dyn Future<Output = Result<Option<AuthenticatedPrincipal>, AnyError>> + Send + '_>,
        > {
            Box::pin(async {
                #[derive(Debug, fundamental_macros::Error)]
                enum StoreError {
                    #[error(code = "STORE_001", message = "keychain locked", status = 500)]
                    Locked,
                }
                Err(AnyError::new(StoreError::Locked))
            })
        }
    }

    /// The bridge's span names the method and request id, never the webview's path.
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn the_invoke_span_records_no_concrete_path() {
        let bridge = InProcessBridge::new(test_router(), ());
        let response = bridge
            .call(BridgeRequest::new(Method::Get, "/api/jan@example.nl"))
            .await
            .expect("transport cannot fail in-process");
        assert_eq!(response.status, 404);
        assert!(logs_contain("native.invoke"), "the span is recorded");
        assert!(logs_contain("request_id="), "and carries this process's id");
        assert!(!logs_contain("jan@example.nl"), "but never the path");
    }

    #[tokio::test]
    async fn dispatches_a_mounted_action_and_echoes_the_body() {
        let bridge = InProcessBridge::new(test_router(), ());
        let response = bridge
            .call(BridgeRequest::new(Method::Post, "/api/echo").with_body(b"{\"n\":1}".to_vec()))
            .await
            .expect("transport cannot fail in-process");
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"{\"n\":1}");
    }

    /// A native request carries its headers and query to the handler.
    #[tokio::test]
    async fn the_headers_and_query_a_request_carries_reach_the_handler() {
        let bridge = InProcessBridge::new(test_router(), ());
        let headers: RequestHeaders = [("Sec-Fetch-Mode".to_owned(), "navigate".to_owned())]
            .into_iter()
            .collect();
        let response = bridge
            .call(
                BridgeRequest::new(Method::Get, "/api/metadata")
                    .with_headers(headers)
                    .with_query("next=/reports"),
            )
            .await
            .expect("transport cannot fail in-process");
        assert_eq!(response.status, 200);
        let seen: String = serde_json::from_slice(&response.body).expect("a json string");
        assert_eq!(seen, "navigate|next=/reports");
    }

    /// A request that states nothing arrives stating nothing.
    #[tokio::test]
    async fn a_request_that_states_nothing_arrives_stating_nothing() {
        let bridge = InProcessBridge::new(test_router(), ());
        let response = bridge
            .call(BridgeRequest::new(Method::Get, "/api/metadata"))
            .await
            .expect("transport cannot fail in-process");
        let seen: String = serde_json::from_slice(&response.body).expect("a json string");
        assert_eq!(seen, "<absent>|");
    }

    #[tokio::test]
    async fn an_unmounted_action_returns_the_routers_404_envelope() {
        let bridge = InProcessBridge::new(test_router(), ());
        let response = bridge
            .call(BridgeRequest::new(Method::Get, "/api/missing"))
            .await
            .expect("a 404 is a response, not a transport error");
        assert_eq!(response.status, 404);
        let envelope: serde_json::Value =
            serde_json::from_slice(&response.body).expect("json error envelope");
        assert_eq!(envelope["status"], 404);
    }

    #[tokio::test]
    async fn attaches_the_resolved_principal_to_dispatch() {
        let bridge = InProcessBridge::new(test_router(), ()).with_principal(FixedPrincipal);
        let response = bridge
            .call(BridgeRequest::new(Method::Get, "/api/whoami"))
            .await
            .expect("in-process");
        assert_eq!(response.body, b"\"user-7\"");
    }

    #[tokio::test]
    async fn defaults_to_anonymous_dispatch() {
        let bridge = InProcessBridge::new(test_router(), ());
        let response = bridge
            .call(BridgeRequest::new(Method::Get, "/api/whoami"))
            .await
            .expect("in-process");
        assert_eq!(response.body, b"\"anonymous\"");
    }

    #[tracing_test::traced_test]
    #[tokio::test]
    async fn dispatch_emits_the_invoke_span_and_resolution_event() {
        let bridge = InProcessBridge::new(test_router(), ());
        bridge
            .call(BridgeRequest::new(Method::Get, "/api/whoami"))
            .await
            .expect("in-process");
        assert!(logs_contain("native.invoke"));
        assert!(logs_contain("principal resolved, dispatching in-process"));
    }

    #[tokio::test]
    async fn a_failing_principal_source_aborts_with_native_001() {
        use fundamental_core::Error as _;
        let bridge = InProcessBridge::new(test_router(), ()).with_principal(FailingPrincipal);
        let err = bridge
            .call(BridgeRequest::new(Method::Get, "/api/whoami"))
            .await
            .expect_err("resolution failure must abort, not downgrade to anonymous");
        assert!(matches!(err, NativeError::Principal { .. }));
        assert_eq!(err.error_code(), "NATIVE_001");
    }
}
