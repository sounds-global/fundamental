//! A streamed route over a real socket.

use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use fundamental_api::{
    AnyError, ApiListener, ApiServer, HttpContext, Method, Middleware, MiddlewareResult, Next,
    RequestBodyStream, Route, Router, ServerConfig, ServerError, ServerListener, StreamBounds,
    StreamedResponse,
};
use fundamental_core::Error as _;
use fundamental_core::address::WebOrigin;
use futures::stream;

#[derive(Clone, Default)]
struct Counted {
    handled: Arc<AtomicUsize>,
}

struct Refuse;

impl<S: Send + Sync + 'static> Middleware<S> for Refuse {
    async fn handle(
        &self,
        _ctx: HttpContext<S>,
        _body: Vec<u8>,
        _next: Next<'_, S>,
    ) -> MiddlewareResult {
        Err(AnyError::new(ServerError::unauthenticated()))
    }
}

struct ChecksBody;

impl<S: Send + Sync + 'static> Middleware<S> for ChecksBody {
    const READS_BODY: bool = true;

    fn handle(
        &self,
        ctx: HttpContext<S>,
        body: Vec<u8>,
        next: Next<'_, S>,
    ) -> impl Future<Output = MiddlewareResult> + Send {
        next.run(ctx, body)
    }
}

async fn count(ctx: HttpContext<Counted>, mut body: RequestBodyStream) -> Result<u64, AnyError> {
    ctx.state().handled.fetch_add(1, Ordering::SeqCst);
    let mut read = 0_u64;
    while let Some(chunk) = body.next_chunk().await {
        read += chunk.map_err(AnyError::new)?.len() as u64;
    }
    Ok(read)
}

async fn download(_: HttpContext<Counted>) -> Result<StreamedResponse, AnyError> {
    let pieces: Vec<Result<Bytes, AnyError>> = (0..4)
        .map(|_| Ok(Bytes::from_static(b"0123456789")))
        .collect();
    Ok(StreamedResponse::new(
        "application/octet-stream",
        40,
        stream::iter(pieces),
    ))
}

async fn download_unknown_length(_: HttpContext<Counted>) -> Result<StreamedResponse, AnyError> {
    let chunks = vec![
        Ok(Bytes::from_static(b"first")),
        Ok(Bytes::from_static(b"second")),
    ];
    Ok(StreamedResponse::with_unknown_length(
        "text/plain",
        stream::iter(chunks),
    ))
}

async fn start(config: ServerConfig, bounds: StreamBounds) -> (ApiListener, Counted) {
    let state = Counted::default();
    let router = Router::new()
        .route(Route::streamed(
            Method::Post,
            "/upload",
            bounds.clone(),
            count,
        ))
        .expect("unique")
        .route(Route::streamed(Method::Post, "/refused", bounds, count).middleware(Refuse))
        .expect("unique")
        .route(Route::post(
            "/json",
            |_: HttpContext<Counted>, (): ()| async move { Ok::<_, AnyError>(()) },
        ))
        .expect("unique")
        .route(Route::streaming_response(
            Method::Get,
            "/download",
            download,
        ))
        .expect("unique");
    let router = router
        .route(Route::streaming_response(
            Method::Get,
            "/unknown-length",
            download_unknown_length,
        ))
        .expect("unique");
    let listener = ApiServer::new(config, state.clone(), router)
        .serve()
        .await
        .expect("serves");
    (listener, state)
}

#[tokio::test]
async fn an_unknown_length_stream_is_sent_without_content_length() {
    let (listener, _) = start(config(), StreamBounds::new(1_000_000)).await;
    let response = reqwest::Client::new()
        .get(format!("http://{}/unknown-length", listener.local_addr()))
        .send()
        .await
        .expect("sent");
    assert_eq!(response.status(), 200);
    assert!(
        !response.headers().contains_key("content-length"),
        "unknown stream length must not be advertised"
    );
    assert_eq!(
        response.bytes().await.expect("body"),
        Bytes::from_static(b"firstsecond")
    );
    listener.stop().await.expect("stops");
}

fn config() -> ServerConfig {
    ServerConfig::new("127.0.0.1:0".parse().expect("addr")).with_max_body_size(64)
}

fn slow_body(chunks: usize, gap: Duration) -> reqwest::Body {
    reqwest::Body::wrap_stream(stream::unfold(0, move |sent| async move {
        if sent == chunks {
            return None;
        }
        tokio::time::sleep(gap).await;
        Some((
            Ok::<_, Infallible>(Bytes::from_static(b"0123456789")),
            sent + 1,
        ))
    }))
}

#[tokio::test]
async fn a_streamed_route_streams_its_body_past_the_buffered_bound() {
    let (listener, state) = start(config(), StreamBounds::new(1_000_000)).await;
    let body = vec![7_u8; 200_000];
    let response = reqwest::Client::new()
        .post(format!("http://{}/upload", listener.local_addr()))
        .body(body)
        .send()
        .await
        .expect("sent");
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.expect("text"), "200000");
    assert_eq!(state.handled.load(Ordering::SeqCst), 1);
    listener.stop().await.expect("stops");
}

#[tokio::test]
async fn a_streamed_route_never_reads_an_unauthorized_body() {
    let (listener, state) = start(config(), StreamBounds::new(1_000_000)).await;
    let started = Instant::now();
    // Ten chunks a quarter-second apart: reading the body would take 2.5 s.
    let response = reqwest::Client::new()
        .post(format!("http://{}/refused", listener.local_addr()))
        .body(slow_body(10, Duration::from_millis(250)))
        .send()
        .await;
    let answered_in = started.elapsed();
    if let Ok(response) = response {
        assert_eq!(response.status(), 401);
    }
    assert_eq!(
        state.handled.load(Ordering::SeqCst),
        0,
        "the handler never ran"
    );
    assert!(
        answered_in < Duration::from_millis(1_500),
        "the refusal waited {answered_in:?} — for the body"
    );
    listener.stop().await.expect("stops");
}

#[tokio::test]
async fn a_body_over_the_route_bound_is_api_020() {
    let (listener, _) = start(config(), StreamBounds::new(1_000)).await;
    let client = reqwest::Client::new();
    let url = format!("http://{}/upload", listener.local_addr());

    // Declared over the bound: refused before a byte is read.
    let response = client
        .post(&url)
        .body(vec![0_u8; 5_000])
        .send()
        .await
        .expect("sent");
    assert_eq!(response.status(), 413);
    let body: serde_json::Value = response.json().await.expect("an envelope");
    assert_eq!(body["error"], "API_020", "{body}");

    // Undeclared (chunked) and over: the handler reads up to the bound and stops.
    let response = client
        .post(&url)
        .body(slow_body(200, Duration::from_millis(1)))
        .send()
        .await
        .expect("sent");
    assert_eq!(response.status(), 413);
    listener.stop().await.expect("stops");
}

#[tokio::test]
async fn a_buffered_route_keeps_the_global_bound() {
    let (listener, _) = start(config(), StreamBounds::new(1_000_000)).await;
    let response = reqwest::Client::new()
        .post(format!("http://{}/json", listener.local_addr()))
        .header("content-type", "application/json")
        .body(vec![b' '; 1_000])
        .send()
        .await
        .expect("sent");
    assert_eq!(response.status(), 413);
    let body: serde_json::Value = response.json().await.expect("an envelope");
    assert_eq!(body["error"], "API_020");
    listener.stop().await.expect("stops");
}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn a_full_streamed_route_is_busy_with_retry_after() {
    let (listener, _) = start(config(), StreamBounds::new(1_000_000).with_max_in_flight(1)).await;
    let url = format!("http://{}/upload", listener.local_addr());
    // The first body is held open until the second has been refused.
    let (held, chunks) = tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(1);
    held.send(Ok(vec![0_u8; 8])).await.expect("sent");
    let holding = tokio::spawn({
        let url = url.clone();
        async move {
            reqwest::Client::new()
                .post(url)
                .body(reqwest::Body::wrap_stream(futures::stream::unfold(
                    chunks,
                    |mut chunks| async move { chunks.recv().await.map(|chunk| (chunk, chunks)) },
                )))
                .send()
                .await
        }
    });
    let mut refused = None;
    for _ in 0..200 {
        let answer = reqwest::Client::new()
            .post(&url)
            .body(vec![1_u8; 10])
            .send()
            .await
            .expect("sent");
        if answer.status() == 503 {
            refused = Some(answer);
            break;
        }
    }
    let refused = refused.expect("a second body while the one slot is held is refused");
    assert_eq!(
        refused
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok()),
        Some("1")
    );
    drop(held);
    let first = holding.await.expect("joined").expect("sent");
    assert_eq!(first.status(), 200, "the first body was read in full");
    listener.stop().await.expect("stops");
}

#[tokio::test]
async fn a_streamed_response_carries_its_length_and_head_keeps_it() {
    let (listener, _) = start(config(), StreamBounds::new(1_000)).await;
    let url = format!("http://{}/download", listener.local_addr());
    let client = reqwest::Client::new();
    let response = client.get(&url).send().await.expect("sent");
    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok()),
        Some("40")
    );
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/octet-stream")
    );
    assert_eq!(response.bytes().await.expect("body").len(), 40);

    let head = client.head(&url).send().await.expect("sent");
    assert_eq!(
        head.headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok()),
        Some("40")
    );
    assert!(head.bytes().await.expect("body").is_empty());
    listener.stop().await.expect("stops");
}

#[tokio::test]
async fn a_body_reading_layer_cannot_guard_a_streamed_route() {
    let router = Router::new()
        .route(
            Route::streamed(Method::Post, "/upload", StreamBounds::new(10), count)
                .middleware(ChecksBody),
        )
        .expect("unique");
    match ApiServer::new(config(), Counted::default(), router)
        .serve()
        .await
    {
        Ok(_) => panic!("a body-reading layer in front of a streamed route started"),
        Err(refused) => assert_eq!(refused.get().error_code(), "API_025"),
    }
}

#[tokio::test]
async fn an_unserved_origin_is_421_on_h1_and_h2() {
    let served = WebOrigin::parse("https://files.example.nl").expect("an origin");
    let (listener, _) = start(
        config().with_served_origins(vec![served]),
        StreamBounds::new(1_000),
    )
    .await;
    let url = format!("http://{}/download", listener.local_addr());
    for client in [
        reqwest::Client::new(),
        reqwest::Client::builder()
            .http2_prior_knowledge()
            .build()
            .expect("an h2 client"),
    ] {
        let wrong = client
            .get(&url)
            .header("host", "app.example.nl")
            .send()
            .await
            .expect("sent");
        assert_eq!(wrong.status(), 421, "{:?}", wrong.version());
        let right = client
            .get(&url)
            .header("host", "files.example.nl")
            .send()
            .await
            .expect("sent");
        assert_eq!(right.status(), 200, "{:?}", right.version());
    }
    listener.stop().await.expect("stops");
}

// CORS answers a preflight without the router, so the refusal must stand in front of it.
#[tokio::test]
async fn an_unserved_origin_is_refused_before_cors_answers_a_preflight() {
    let served = WebOrigin::parse("https://files.example.nl").expect("an origin");
    let (listener, counted) = start(
        config()
            .with_served_origins(vec![served])
            .with_cors_allowed_origins(vec!["https://app.example.nl".to_owned()]),
        StreamBounds::new(1_000),
    )
    .await;
    let url = format!("http://{}/json", listener.local_addr());
    let client = reqwest::Client::new();
    let preflight = |host: &'static str| {
        client
            .request(reqwest::Method::OPTIONS, &url)
            .header("host", host)
            .header("origin", "https://app.example.nl")
            .header("access-control-request-method", "POST")
            .send()
    };

    let admitted = preflight("files.example.nl").await.expect("sent");
    assert!(
        admitted
            .headers()
            .contains_key("access-control-allow-origin"),
        "a served host is preflighted as before: {:?}",
        admitted.headers()
    );

    let refused = preflight("app.example.nl").await.expect("sent");
    assert_eq!(refused.status(), 421);
    assert!(
        !refused
            .headers()
            .keys()
            .any(|name| name.as_str().starts_with("access-control-")),
        "{:?}",
        refused.headers()
    );
    let body = refused.text().await.expect("text");
    assert!(
        body.contains("API_024") && !body.contains("<html"),
        "{body}"
    );

    let head = client
        .head(&url)
        .header("host", "app.example.nl")
        .send()
        .await
        .expect("sent");
    assert_eq!(head.status(), 421);
    assert!(head.bytes().await.expect("bytes").is_empty());
    assert_eq!(counted.handled.load(Ordering::SeqCst), 0);
    listener.stop().await.expect("stops");
}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn a_head_never_polls_a_streamed_response() {
    let polled = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&polled);
    let router = Router::new()
        .route(Route::streaming_response(
            Method::Get,
            "/file",
            move |_: HttpContext<Counted>| {
                let seen = Arc::clone(&seen);
                async move {
                    let body = stream::poll_fn(move |_| {
                        seen.fetch_add(1, Ordering::SeqCst);
                        std::task::Poll::Ready(None::<Result<Bytes, AnyError>>)
                    });
                    Ok::<_, AnyError>(StreamedResponse::new("text/plain", 0, body))
                }
            },
        ))
        .expect("unique");
    let listener = ApiServer::new(config(), Counted::default(), router)
        .serve()
        .await
        .expect("serves");
    let head = reqwest::Client::new()
        .head(format!("http://{}/file", listener.local_addr()))
        .send()
        .await
        .expect("sent");
    assert_eq!(head.status(), 200);
    assert_eq!(polled.load(Ordering::SeqCst), 0, "a HEAD produced the body");
    listener.stop().await.expect("stops");
}
