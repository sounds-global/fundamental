//! The accepted connection's address reaches the handler, over a real socket.

use std::net::SocketAddr;

use fundamental_api::ApiServer;
use fundamental_core::AnyError;
use fundamental_server::{HttpContext, Route, Router, ServerConfig, ServerListener};

async fn peer(context: HttpContext<()>, _: ()) -> Result<String, AnyError> {
    Ok(context
        .peer()
        .map_or_else(|| "none".to_owned(), |peer| peer.to_string()))
}

#[tokio::test]
async fn a_handler_is_told_which_socket_the_request_arrived_on() {
    let router = Router::<()>::new()
        .route(Route::get("/peer", peer))
        .expect("the route registers");
    let listener = ApiServer::new(
        ServerConfig::new("127.0.0.1:0".parse().expect("an ephemeral bind")),
        (),
        router,
    )
    .serve()
    .await
    .expect("binds an ephemeral port");

    let body = reqwest::get(format!("http://{}/peer", listener.local_addr()))
        .await
        .expect("sent")
        .text()
        .await
        .expect("body");

    // The action renders a `String` as JSON, so the address arrives quoted.
    let seen: SocketAddr = body.trim_matches('"').parse().unwrap_or_else(|_| {
        panic!("the handler saw no peer, so connect-info is not wired: {body}")
    });
    assert!(
        seen.ip().is_loopback(),
        "the peer is not this test's own connection: {seen}"
    );
    assert_ne!(
        seen.port(),
        listener
            .local_addr()
            .address()
            .expect("a bound address")
            .port(),
        "the handler was handed the listener's own address instead of the caller's"
    );

    listener.stop().await.expect("stops");
}
