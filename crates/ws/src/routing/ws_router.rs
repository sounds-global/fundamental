//! [`WsRouter`]: maps an inbound action to the handler that serves it.

use std::collections::HashMap;

use serde_json::Value;

use crate::context::WsContext;
use crate::error::{WsError, WsResult};
use crate::routing::Route;
use crate::routing::ws_handler::ErasedHandler;

/// Routes inbound messages to handlers by their `action` string.
#[derive(Default)]
pub struct WsRouter {
    routes: HashMap<String, Box<dyn ErasedHandler>>,
}

impl WsRouter {
    /// Creates an empty router.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a route, replacing any existing handler for the same action.
    #[must_use]
    pub fn route(mut self, route: Route) -> Self {
        self.routes.insert(route.action, route.handler);
        self
    }

    /// The actions this router serves, in unspecified order.
    pub fn actions(&self) -> impl Iterator<Item = &str> {
        self.routes.keys().map(String::as_str)
    }

    /// The route's own name for `action`: the server's string, safe for a span.
    pub(crate) fn declared(&self, action: &str) -> Option<&str> {
        self.routes
            .get_key_value(action)
            .map(|(name, _)| name.as_str())
    }

    /// Dispatches `data` to the handler registered for `action`.
    pub(crate) async fn dispatch(
        &self,
        action: &str,
        data: Value,
        context: WsContext,
    ) -> WsResult<()> {
        match self.routes.get(action) {
            Some(handler) => handler.call(context, data).await,
            None => Err(WsError::UnknownAction),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::WsError;
    use fundamental_core::Error as _;
    use serde::Deserialize;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Deserialize)]
    struct Ping {
        n: u64,
    }

    async fn context() -> WsContext {
        crate::context::test_support::anonymous_context().await.0
    }

    #[tokio::test]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn dispatch_runs_the_registered_handler() {
        let total = Arc::new(AtomicU64::new(0));
        let seen = Arc::clone(&total);
        let router = WsRouter::new().route(Route::new("ping", move |_ctx, ping: Ping| {
            let seen = Arc::clone(&seen);
            async move {
                seen.fetch_add(ping.n, Ordering::SeqCst);
                Ok(())
            }
        }));

        router
            .dispatch("ping", serde_json::json!({ "n": 5 }), context().await)
            .await
            .expect("handler runs");
        assert_eq!(total.load(Ordering::SeqCst), 5);
    }

    #[tokio::test]
    async fn unknown_action_is_rejected_with_code() {
        let router = WsRouter::new();
        let err = router
            .dispatch("nope", Value::Null, context().await)
            .await
            .expect_err("unknown action");
        assert_eq!(err.error_code(), "WS_003");
        assert!(matches!(err, WsError::UnknownAction));
    }

    #[test]
    fn only_a_declared_action_has_a_name_to_record() {
        let router =
            WsRouter::new().route(Route::new("ping", |_ctx, _ping: Ping| async { Ok(()) }));
        assert_eq!(router.declared("ping"), Some("ping"));
        assert_eq!(router.declared("jan@example.nl"), None);
    }

    #[tokio::test]
    async fn malformed_payload_is_an_invalid_message() {
        let router =
            WsRouter::new().route(Route::new("ping", |_ctx, _ping: Ping| async { Ok(()) }));
        let err = router
            .dispatch("ping", serde_json::json!({ "n": "oops" }), context().await)
            .await
            .expect_err("payload shape mismatch");
        assert_eq!(err.error_code(), "WS_002");
    }
}
