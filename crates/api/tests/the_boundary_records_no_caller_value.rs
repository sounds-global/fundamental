//! The HTTP boundary records no byte string a caller chose; alone in its binary for the global subscriber.

use fundamental_api::{
    AnyError, ApiServer, HttpContext, Router, ServerConfig, ServerListener, ServerResult,
};
use fundamental_auth::{AuthError, AuthenticatedPrincipal, InvalidToken, PrincipalVerifier};
use fundamental_core::secrets::SecretString;
use fundamental_macros::action;
use fundamental_observability::testing::CapturedLogs;
use fundamental_server::{CredentialSource, Locale, LocalePolicy};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SpanData};
use serde::{Deserialize, Serialize};
use tracing_subscriber::layer::SubscriberExt;

type _Ctx = HttpContext<AppState>;

const EMAIL: &str = "jan@example.nl";
const TOKEN: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
const UUID_TOKEN: &str = "6f1c3a5e-8d2b-4c7a-9e0f-1b2c3d4e5f60";
const INBOUND_TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
const SESSION_COOKIE: &str = "__Host-session";

fn needles() -> Vec<String> {
    vec![
        EMAIL.to_owned(),
        "jan%40example.nl".to_owned(),
        TOKEN.to_owned(),
        UUID_TOKEN.to_owned(),
        "q".repeat(32),
        "state-secret".to_owned(),
    ]
}

#[derive(Clone)]
struct AppState;

#[derive(Serialize, Deserialize)]
struct Greeting {
    text: String,
}

#[derive(Serialize, Deserialize)]
struct Proof {
    assertion: String,
}

/// The query a lookup accepts, so a caller's values reach the handler.
#[derive(Serialize, Deserialize)]
struct Lookup {
    email: Option<String>,
    t: Option<String>,
}

#[action(GET, "/users/{id}")]
async fn user(ctx: HttpContext<AppState>, input: Lookup) -> Result<Greeting, AnyError> {
    let asked = usize::from(input.email.is_some()) + usize::from(input.t.is_some());
    Ok(Greeting {
        text: format!("user {} ({asked} filters)", ctx.params().require("id")?),
    })
}

#[action(POST, "/session")]
async fn session(_ctx: HttpContext<AppState>, input: Proof) -> Result<Greeting, AnyError> {
    Ok(Greeting {
        text: input.assertion,
    })
}

/// A handler that panics on a message quoting request data.
#[action(GET, "/fragile/{value}")]
async fn fragile(ctx: HttpContext<AppState>) -> Result<Greeting, AnyError> {
    let value = ctx.params().require("value")?.to_owned();
    std::panic::panic_any(format!("could not handle {value}"))
}

struct RejectingVerifier;

impl PrincipalVerifier for RejectingVerifier {
    async fn verify(&self, _token: &SecretString) -> Result<AuthenticatedPrincipal, AuthError> {
        Err(AuthError::Invalid(InvalidToken::Unrecognised))
    }
}

fn router() -> ServerResult<Router<AppState>> {
    Router::new()
        .route(user_route())?
        .route(session_route())?
        .route(fragile_route())
}

/// Installs a subscriber that formats every span and event, and exports every span to memory.
fn install() -> (CapturedLogs, InMemorySpanExporter, SdkTracerProvider) {
    let captured = CapturedLogs::default();
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(captured.layer())
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("boundary")))
        .with(tracing_subscriber::filter::LevelFilter::TRACE);
    tracing::subscriber::set_global_default(subscriber).expect("the only subscriber");
    (captured, exporter, provider)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn no_value_a_caller_chose_is_recorded_at_the_boundary() {
    let (captured, exporter, provider) = install();
    assert!(fundamental_observability::trace_context::is_propagating());

    let listener = ApiServer::new(
        ServerConfig::new("127.0.0.1:0".parse().expect("a loopback address"))
            .with_credential_source(CredentialSource::cookie(SESSION_COOKIE))
            // Negotiating a language is what makes the boundary read `Accept-Language`.
            .with_locale_policy(LocalePolicy::new(
                "en".parse::<Locale>().expect("a language tag"),
            )),
        AppState,
        router().expect("the routes are unique"),
    )
    .with_verifier(RejectingVerifier)
    .serve()
    .await
    .expect("binds an ephemeral port");
    let base = format!("http://{}", listener.local_addr());
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("a client");
    let oversized = "q".repeat(10_000);

    // A matched route, carrying a hostile id, trace state, language and query.
    let matched = client
        .get(format!("{base}/users/42?email={EMAIL}&t={TOKEN}"))
        .header("x-request-id", UUID_TOKEN)
        .header(
            "traceparent",
            format!("00-{INBOUND_TRACE}-00f067aa0ba902b7-01"),
        )
        .header("tracestate", format!("x=state-secret,y={EMAIL}"))
        .header(
            "accept-language",
            format!("{EMAIL}, {TOKEN};q=0.5, {UUID_TOKEN};q=high"),
        )
        .send()
        .await
        .expect("the matched request is answered");
    let matched_status = matched.status();
    let echoed_header = matched.headers().get("x-request-id").cloned();
    let matched_body = matched.text().await.unwrap_or_default();
    assert_eq!(matched_status, 200, "{matched_body}");
    let echoed = echoed_header
        .as_ref()
        .and_then(|value| value.to_str().ok())
        .expect("the recorded id is echoed")
        .to_owned();
    assert_ne!(
        echoed, UUID_TOKEN,
        "a caller's id is never recorded by default"
    );
    assert_eq!(
        echoed.len(),
        36,
        "the echoed id is this server's UUID: {echoed}"
    );

    // An unmatched path, with an oversized id and a malformed trace context.
    let unmatched = client
        .get(format!("{base}/reset/{EMAIL}/{TOKEN}?email={EMAIL}"))
        .header("x-request-id", &oversized)
        .header("traceparent", format!("00-{oversized}"))
        .send()
        .await
        .expect("the unmatched request is answered");
    assert_eq!(unmatched.status(), 404);
    let body = unmatched.text().await.expect("a 404 body");
    assert!(
        body.contains("{email}/{hex:33+}"),
        "the 404 names the shape: {body}"
    );

    // An extension method that is itself a token.
    let extension = client
        .request(
            reqwest::Method::from_bytes(TOKEN.to_uppercase().as_bytes()).expect("a method token"),
            format!("{base}/users/42"),
        )
        .send()
        .await
        .expect("the extension method is answered");
    assert_eq!(extension.status(), 405);

    // Ten kilobytes in a path segment, and bytes outside visible ASCII in every
    // header the boundary reads.
    let padded = client
        .get(format!("{base}/{oversized}/{EMAIL}"))
        .header(
            "x-request-id",
            reqwest::header::HeaderValue::from_bytes(format!("\u{ff}{EMAIL}").as_bytes())
                .expect("obs-text is a valid header value"),
        )
        .header(
            "traceparent",
            reqwest::header::HeaderValue::from_bytes(format!("00-\u{ff}{TOKEN}").as_bytes())
                .expect("obs-text is a valid header value"),
        )
        .send()
        .await
        .expect("the padded request is answered");
    assert_eq!(padded.status(), 404);

    // Three cross-site writes on a cookie surface: a hostile Origin, an oversized
    // one, and a hostile Sec-Fetch-Site.
    for (name, value) in [
        ("origin", format!("https://{EMAIL}/{TOKEN}")),
        ("origin", format!("https://{oversized}.example")),
        ("sec-fetch-site", TOKEN.to_owned()),
    ] {
        let refused = client
            .post(format!("{base}/session"))
            .header(name, value)
            .json(&Proof {
                assertion: "unread".to_owned(),
            })
            .send()
            .await
            .expect("the write is answered");
        assert_eq!(refused.status(), 403);
        let body = refused.text().await.expect("a 403 body");
        assert!(!body.contains(EMAIL) && !body.contains(TOKEN), "{body}");
    }

    // A handler panicking on a message that quotes the caller's value.
    let panicked = client
        .get(format!("{base}/fragile/{EMAIL}"))
        .send()
        .await
        .expect("the panic is answered");
    assert_eq!(panicked.status(), 500);

    provider
        .force_flush()
        .expect("the in-memory exporter flushes");
    let logs = captured.text();
    let spans: Vec<SpanData> = exporter.get_finished_spans().expect("the exported spans");
    let exported = format!("{spans:?}");

    for needle in needles() {
        let needle = needle.to_lowercase();
        assert!(
            !logs.to_lowercase().contains(&needle),
            "a caller's value reached the logs: {needle}\n{logs}"
        );
        assert!(
            !exported.to_lowercase().contains(&needle),
            "a caller's value reached an exported span: {needle}\n{exported}"
        );
    }

    // What the boundary does record, so a green run is not an empty one.
    let logs = logs.replace('"', "");
    assert!(
        logs.contains("Accept-Language entry"),
        "the language entries were read:\n{logs}"
    );
    assert!(
        logs.contains(&echoed),
        "the recorded id is in the logs:\n{logs}"
    );
    assert!(
        logs.contains("path=/{text:1-8}/{email}/{hex:33+}"),
        "the unmatched path is recorded as its shape:\n{logs}"
    );
    assert!(logs.contains("method=extension"), "{logs}");
    assert!(logs.contains("rejected=oversized"), "{logs}");
    let refusals: Vec<&str> = logs
        .lines()
        .filter(|line| line.contains("refused a cookie-authenticated write"))
        .collect();
    assert_eq!(refusals.len(), 3, "every refusal is logged:\n{logs}");
    for line in refusals {
        assert!(
            line.contains("request_id="),
            "a boundary refusal carries this server's request id: {line}"
        );
    }
    assert!(logs.contains("a request handler panicked"), "{logs}");

    // The default trace policy is a link: the caller's trace is named, not joined.
    let outer = spans
        .iter()
        .find(|span| {
            span.name == "http.request"
                && span
                    .links
                    .iter()
                    .any(|link| link.span_context.trace_id().to_string() == INBOUND_TRACE)
        })
        .expect("the traced request's outermost span links the caller's context");
    let own_trace = outer.span_context.trace_id();
    assert_ne!(own_trace.to_string(), INBOUND_TRACE);
    assert_eq!(outer.span_context.trace_state().header(), "");
    for link in outer.links.iter() {
        assert_eq!(link.span_context.trace_state().header(), "");
    }
    assert!(
        spans
            .iter()
            .any(|span| span.name == "server.dispatch" && span.span_context.trace_id() == own_trace),
        "the router's span is part of the request's own trace"
    );

    drop(listener);
}
