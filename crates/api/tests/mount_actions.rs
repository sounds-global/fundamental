//! `mount_actions!` generates a `register_actions` that mounts every listed action.

use fundamental_api::{AnyError, ApiServer, Router, ServerConfig, ServerListener};
use fundamental_macros::{action, mount_actions};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct AppState;

#[derive(Serialize, Deserialize)]
struct Echo {
    msg: String,
}

#[action(POST, "/api/greet")]
async fn greet(_ctx: HttpContext<AppState>, input: Echo) -> Result<Echo, AnyError> {
    Ok(Echo {
        msg: message_prefixed(&input.msg, "hello"),
    })
}

#[action(POST, "/api/farewell")]
async fn farewell(_ctx: HttpContext<AppState>, input: Echo) -> Result<Echo, AnyError> {
    Ok(Echo {
        msg: message_prefixed(&input.msg, "bye"),
    })
}

fn message_prefixed(msg: &str, prefix: &str) -> String {
    format!("{prefix}, {msg}")
}

mount_actions! { Greet, Farewell }

#[test]
fn register_actions_mounts_every_listed_action() {
    let router = register_actions(Router::<AppState>::new()).expect("actions mount");
    assert_eq!(router.len(), 2, "both Greet and Farewell are mounted");
}

#[tokio::test]
async fn a_mounted_action_round_trips_over_http() {
    let router = register_actions(Router::<AppState>::new()).expect("actions mount");
    let server = ApiServer::new(
        ServerConfig::new("127.0.0.1:0".parse().unwrap()),
        AppState,
        router,
    );
    let listener = server.serve().await.expect("serve");
    let addr = listener.local_addr().address().expect("a bound address");

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/greet"))
        .json(&serde_json::json!({ "msg": "world" }))
        .send()
        .await
        .expect("request");
    let body: Echo = response.json().await.expect("json");
    assert_eq!(body.msg, "hello, world");

    listener.stop().await.expect("stop");
}
