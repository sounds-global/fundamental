//! Under `InboundTraceContext::Adopt`, a request is one trace; alone in its binary for the global subscriber.

use fundamental_api::{
    AnyError, ApiServer, HttpContext, InboundTraceContext, Router, ServerConfig, ServerListener,
    ServerResult,
};
use fundamental_macros::action;
use opentelemetry::trace::{SpanId, TracerProvider as _};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
use serde::{Deserialize, Serialize};
use tracing_subscriber::layer::SubscriberExt;

type _Ctx = HttpContext<AppState>;

const INBOUND_TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
const INBOUND_SPAN: &str = "00f067aa0ba902b7";

#[derive(Clone)]
struct AppState;

#[derive(Serialize, Deserialize)]
struct Pong {
    ok: bool,
}

#[action(GET, "/ping")]
async fn ping(_ctx: HttpContext<AppState>) -> Result<Pong, AnyError> {
    Ok(Pong { ok: true })
}

fn router() -> ServerResult<Router<AppState>> {
    Router::new().route(ping_route())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn a_trusted_peer_s_request_is_one_trace() {
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("peer")))
            .with(tracing_subscriber::filter::LevelFilter::INFO),
    )
    .expect("the only subscriber");

    let listener = ApiServer::new(
        ServerConfig::new("127.0.0.1:0".parse().expect("a loopback address"))
            .with_inbound_trace_context(InboundTraceContext::Adopt),
        AppState,
        router().expect("the route is unique"),
    )
    .serve()
    .await
    .expect("binds an ephemeral port");

    let response = reqwest::Client::new()
        .get(format!("http://{}/ping", listener.local_addr()))
        .header(
            "traceparent",
            format!("00-{INBOUND_TRACE}-{INBOUND_SPAN}-01"),
        )
        .send()
        .await
        .expect("the request is answered");
    assert_eq!(response.status(), 200);

    provider
        .force_flush()
        .expect("the in-memory exporter flushes");
    let spans = exporter.get_finished_spans().expect("the exported spans");

    let outer = spans
        .iter()
        .find(|span| span.name == "http.request")
        .expect("the outermost span is exported");
    assert_eq!(
        outer.span_context.trace_id().to_string(),
        INBOUND_TRACE,
        "the outermost span joins the trusted caller's trace"
    );
    assert_eq!(outer.parent_span_id.to_string(), INBOUND_SPAN);

    let dispatch = spans
        .iter()
        .find(|span| span.name == "server.dispatch")
        .expect("the router's span is exported");
    assert_eq!(
        dispatch.span_context.trace_id(),
        outer.span_context.trace_id()
    );
    assert_eq!(
        dispatch.parent_span_id,
        outer.span_context.span_id(),
        "the router's span is a child of the outermost one"
    );

    assert!(
        spans
            .iter()
            .all(|span| span.span_context.trace_id().to_string() == INBOUND_TRACE),
        "every span of the request is in one trace"
    );
    assert_ne!(outer.parent_span_id, SpanId::INVALID);

    drop(listener);
}
