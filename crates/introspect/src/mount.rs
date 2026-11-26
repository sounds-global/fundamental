use fundamental_nats::MessageDescriptor;
use fundamental_server::{ActionDescriptor, HttpContext, Route, Router, ServerResult};
use tracing::{debug, instrument};

use crate::async_api_document::AsyncApiDocument;
use crate::doc_page::DocPage;
use crate::drain::Drain;
use crate::dto::SurfaceDto;
use crate::open_api_document::OpenApiDocument;
use crate::paths::{ASYNCAPI_PATH, LOGS_PATH, OPENAPI_PATH, SURFACE_PATH, TRACES_PATH};

/// Builds an app's introspection surface and mounts it as JSON on the app's HTTP router.
///
/// ```
/// # #[cfg(feature = "ssr")] {
/// use fundamental_introspect::{DocPage, Introspection};
/// use fundamental_server::Router;
///
/// let router: Router<()> = Introspection::new("demo", "1.0.0")
///     .page(DocPage::new("Overview", "<p>What this service does.</p>").group("Guides"))
///     .mount(Router::new())
///     .expect("introspection mounts");
/// # let _ = router;
/// # }
/// ```
#[must_use]
pub struct Introspection {
    title: String,
    version: String,
    actions: Vec<ActionDescriptor>,
    messages: Vec<MessageDescriptor>,
    pages: Vec<DocPage>,
    drain: Option<Drain>,
}

impl Introspection {
    /// Starts an introspection surface for the app named `title` at API version `version`.
    pub fn new(title: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            version: version.into(),
            actions: Vec::new(),
            messages: Vec::new(),
            pages: Vec::new(),
            drain: None,
        }
    }

    /// Exposes a log and trace [`Drain`]; it must also be installed as a `tracing` layer.
    pub fn drain(mut self, drain: Drain) -> Self {
        self.drain = Some(drain);
        self
    }

    /// Sets the HTTP actions to document (typically `describe_actions()`).
    pub fn actions(mut self, actions: Vec<ActionDescriptor>) -> Self {
        self.actions = actions;
        self
    }

    /// Sets the messages to document (typically `describe_messages()`).
    pub fn messages(mut self, messages: Vec<MessageDescriptor>) -> Self {
        self.messages = messages;
        self
    }

    /// Sets all authored documentation pages at once.
    pub fn pages(mut self, pages: Vec<DocPage>) -> Self {
        self.pages = pages;
        self
    }

    /// Adds one authored documentation page.
    pub fn page(mut self, page: DocPage) -> Self {
        self.pages.push(page);
        self
    }

    /// Mounts the surface and both spec documents as JSON routes on `router`.
    ///
    /// # Errors
    /// Returns the [`ServerError`](fundamental_server::ServerError) from
    /// [`Router::route`](fundamental_server::Router::route) if an introspection path collides
    /// with an existing route.
    #[instrument(
        skip(self, router),
        fields(app = %self.title, actions = self.actions.len(), messages = self.messages.len(), pages = self.pages.len())
    )]
    pub fn mount<S>(self, router: Router<S>) -> ServerResult<Router<S>>
    where
        S: Send + Sync + 'static,
    {
        let Introspection {
            title,
            version,
            actions,
            messages,
            pages,
            drain,
        } = self;
        let (action_count, message_count, page_count) =
            (actions.len(), messages.len(), pages.len());

        // `surface.json` borrows the descriptors, so it is built before they move into the specs.
        let surface = SurfaceDto::build(&title, &version, &pages, &actions, &messages);
        let openapi = OpenApiDocument::new(&title, &version, actions);
        let asyncapi = AsyncApiDocument::new(&title, &version, messages);

        let router = router
            .route(json_route(SURFACE_PATH, surface))?
            .route(json_route(OPENAPI_PATH, openapi))?
            .route(json_route(ASYNCAPI_PATH, asyncapi))?;
        let has_drain = drain.is_some();
        let router = match drain {
            Some(drain) => drain_routes(router, drain)?,
            None => router,
        };

        log_mounted([action_count, message_count, page_count], has_drain);
        Ok(router)
    }
}

/// Mounts the introspection surface on `router`; what `mount_actions!` calls.
///
/// # Errors
/// Returns the [`ServerError`](fundamental_server::ServerError) from
/// [`Router::route`](fundamental_server::Router::route) if an introspection path collides with
/// an existing route.
pub fn mount<S>(
    router: Router<S>,
    title: &str,
    version: &str,
    actions: Vec<ActionDescriptor>,
    messages: Vec<MessageDescriptor>,
) -> ServerResult<Router<S>>
where
    S: Send + Sync + 'static,
{
    Introspection::new(title, version)
        .actions(actions)
        .messages(messages)
        .mount(router)
}

/// The one event saying what was mounted.
fn log_mounted([actions, messages, pages]: [usize; 3], drain: bool) {
    debug!(
        actions,
        messages,
        pages,
        drain,
        surface = SURFACE_PATH,
        openapi = OPENAPI_PATH,
        asyncapi = ASYNCAPI_PATH,
        "mounted introspection surface"
    );
}

/// The log and trace routes, snapshotting the drain on each request.
fn drain_routes<S>(router: Router<S>, drain: Drain) -> ServerResult<Router<S>>
where
    S: Send + Sync + 'static,
{
    let logs = drain.clone();
    let traces = drain;
    router
        .route(Route::get(
            LOGS_PATH,
            move |_ctx: HttpContext<S>, _input: ()| {
                let logs = logs.clone();
                async move { Ok(logs.logs()) }
            },
        ))?
        .route(Route::get(
            TRACES_PATH,
            move |_ctx: HttpContext<S>, _input: ()| {
                let traces = traces.clone();
                async move { Ok(traces.spans()) }
            },
        ))
}

fn json_route<S, T>(path: &'static str, value: T) -> Route<S>
where
    S: Send + Sync + 'static,
    T: serde::Serialize + Clone + Send + Sync + 'static,
{
    Route::get(path, move |_ctx: HttpContext<S>, _input: ()| {
        let value = value.clone();
        async move { Ok(value) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_nats::MessageTransport;
    use fundamental_schema::SchemaNode;
    use fundamental_server::{Method, ServerRequest};

    fn built() -> Introspection {
        Introspection::new("Demo", "1.0.0")
            .actions(vec![ActionDescriptor::new(
                "POST",
                "/users",
                "Create a user.",
                &["auth"],
                None,
                SchemaNode::Any,
                SchemaNode::Any,
            )])
            .messages(vec![MessageDescriptor::new(
                "events.user.created",
                MessageTransport::Nats,
                "",
                SchemaNode::Any,
            )])
            .page(DocPage::new("Overview", "<p>Welcome.</p>").group("Guides"))
    }

    async fn response_at(path: &str) -> (u16, String, Vec<u8>) {
        // `tracing` caches a callsite's `Interest` process-wide from the first dispatcher it meets.
        let _guard = tracing::subscriber::set_default(tracing_subscriber::registry());
        let router = built()
            .mount(Router::<()>::new())
            .expect("introspection mounts");
        let request = ServerRequest::new(Method::Get, path.to_string(), ());
        let response = router.dispatch(request).await;
        (
            response.status(),
            response.content_type().to_string(),
            response.body().to_vec(),
        )
    }

    #[tokio::test]
    async fn serves_the_surface_with_actions_messages_and_pages() {
        let (status, content_type, body) = response_at(SURFACE_PATH).await;
        assert_eq!(status, 200);
        assert!(content_type.starts_with("application/json"));
        let surface: SurfaceDto = serde_json::from_slice(&body).expect("surface json");
        assert_eq!(surface.app, "Demo");
        assert_eq!(surface.version, "1.0.0");
        assert_eq!(surface.actions[0].path, "/users");
        assert_eq!(surface.messages[0].subject, "events.user.created");
        assert_eq!(surface.pages[0].title, "Overview");
    }

    #[tokio::test]
    async fn serves_the_spec_documents() {
        let (_, _, openapi) = response_at(OPENAPI_PATH).await;
        let doc: serde_json::Value = serde_json::from_slice(&openapi).expect("json");
        assert_eq!(doc["openapi"], "3.1.0");
        assert!(doc["paths"]["/users"]["post"].is_object());

        let (_, _, asyncapi) = response_at(ASYNCAPI_PATH).await;
        let doc: serde_json::Value = serde_json::from_slice(&asyncapi).expect("json");
        assert_eq!(doc["asyncapi"], "3.0.0");
    }

    #[test]
    fn the_free_mount_fn_forwards_to_the_builder() {
        let router = mount(
            Router::<()>::new(),
            "Demo",
            "1.0.0",
            vec![ActionDescriptor::new(
                "GET",
                "/ping",
                "",
                &[],
                None,
                SchemaNode::Any,
                SchemaNode::Any,
            )],
            Vec::new(),
        )
        .expect("introspection mounts");
        assert!(!router.is_empty());
    }

    // Single-threaded so the span stays on this subscriber across the await.
    #[tokio::test(flavor = "current_thread")]
    async fn a_poll_of_the_surface_is_not_captured_by_its_own_drain() {
        use tracing_subscriber::prelude::*;

        use crate::drain::INTROSPECT_PREFIX;
        use crate::paths::{OPENAPI_PATH, TRACES_PATH};

        let drain = Drain::new();
        let subscriber = tracing_subscriber::registry().with(drain.clone());
        let _guard = tracing::subscriber::set_default(subscriber);

        let router = built()
            .drain(drain.clone())
            .mount(Router::<()>::new())
            .expect("introspection mounts");

        for path in [TRACES_PATH, OPENAPI_PATH] {
            let request = ServerRequest::new(Method::Get, path.to_string(), ());
            assert_eq!(router.dispatch(request).await.status(), 200);
        }
        // The positive control: an unrouted request still records its `path` shape.
        let request = ServerRequest::new(Method::Get, "/ping".to_string(), ());
        assert_eq!(router.dispatch(request).await.status(), 404);
        assert!(
            drain.spans().iter().any(|span| span
                .fields
                .iter()
                .any(|(name, value)| name == "path" && !value.contains(INTROSPECT_PREFIX))),
            "the drain captured no unrouted request, so every assertion below is vacuous"
        );

        let captured = drain.spans();
        assert!(
            captured.iter().all(|span| !span
                .fields
                .iter()
                .any(|(_, value)| value.contains(INTROSPECT_PREFIX))),
            "polling the introspection surface captured its own request spans: {captured:?}"
        );
    }
}
