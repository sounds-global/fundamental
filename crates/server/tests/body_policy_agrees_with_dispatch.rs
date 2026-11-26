//! The backend reads a body under the policy of the route dispatch then runs: both come from one lookup.

use fundamental_core::AnyError;
use fundamental_server::{
    BodyPolicy, HttpContext, Method, RequestBodyStream, Route, Router, ServerRequest, StreamBounds,
};

#[derive(Clone)]
struct State;

async fn streamed(_: HttpContext<State>, mut body: RequestBodyStream) -> Result<String, AnyError> {
    let mut read = 0;
    while let Some(chunk) = body.next_chunk().await {
        read += chunk.map_err(AnyError::new)?.len();
    }
    Ok(format!("streamed {read}"))
}

async fn buffered(_: HttpContext<State>, (): ()) -> Result<String, AnyError> {
    Ok("buffered".to_owned())
}

fn router() -> Router<State> {
    Router::new()
        .route(Route::streamed(
            Method::Post,
            "/teams/{team}/files",
            StreamBounds::new(100),
            streamed,
        ))
        .expect("unique")
        .route(Route::post("/teams/{team}/notes", buffered))
        .expect("unique")
        .route(Route::get("/teams/{team}/files", buffered))
        .expect("unique")
}

#[tokio::test]
async fn body_policy_and_dispatch_agree_for_every_route() {
    let router = router();
    let cases = [
        (Method::Post, "/teams/7/files", true),
        (Method::Get, "/teams/7/files", false),
        (Method::Post, "/teams/7/notes", false),
        (Method::Post, "/nowhere", false),
        (Method::Put, "/teams/7/files", false),
    ];
    for (method, path, expect_streamed) in cases {
        let request = ServerRequest::new(method, path, State);
        let streamed_policy = matches!(router.body_policy(&request), BodyPolicy::Streamed(_));
        assert_eq!(streamed_policy, expect_streamed, "{method} {path}");

        let response = router
            .dispatch(ServerRequest::new(method, path, State).with_body(b"abc".to_vec()))
            .await;
        let body = String::from_utf8_lossy(response.body()).into_owned();
        assert_eq!(
            body.contains("streamed"),
            expect_streamed,
            "{method} {path} answered {body}"
        );
    }
}

#[tokio::test]
async fn a_projected_streamed_route_keeps_its_policy() {
    #[derive(Clone)]
    struct Outer(State);
    impl fundamental_server::FromRef<Outer> for State {
        fn from_ref(outer: &Outer) -> Self {
            outer.0.clone()
        }
    }
    let route: Route<Outer> =
        Route::streamed(Method::Post, "/upload", StreamBounds::new(10), streamed).project();
    let router = Router::new().route(route).expect("unique");
    let request = ServerRequest::new(Method::Post, "/upload", Outer(State));
    assert!(matches!(
        router.body_policy(&request),
        BodyPolicy::Streamed(_)
    ));
}
