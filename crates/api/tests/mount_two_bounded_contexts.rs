//! Two bounded contexts on one router, each handler given its own state through `FromRef`.

use fundamental_api::{AnyError, ApiServer, Router, ServerConfig, ServerListener};
use fundamental_macros::{action, mount_actions};
use fundamental_server::FromRef;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct Identity {
    greeting: &'static str,
}

#[derive(Clone)]
struct Organisations {
    company: &'static str,
}

#[derive(Clone)]
struct Portal {
    identity: Identity,
    organisations: Organisations,
}

impl FromRef<Portal> for Identity {
    fn from_ref(portal: &Portal) -> Self {
        portal.identity.clone()
    }
}

impl FromRef<Portal> for Organisations {
    fn from_ref(portal: &Portal) -> Self {
        portal.organisations.clone()
    }
}

#[derive(Serialize, Deserialize)]
struct Answer {
    said: String,
}

#[action(POST, "/who")]
async fn who(ctx: HttpContext<Identity>, _input: ()) -> Result<Answer, AnyError> {
    Ok(Answer {
        said: ctx.state().greeting.to_owned(),
    })
}

#[action(POST, "/where")]
async fn where_at(ctx: HttpContext<Organisations>, _input: ()) -> Result<Answer, AnyError> {
    Ok(Answer {
        said: ctx.state().company.to_owned(),
    })
}

mount_actions! { Who, WhereAt }

fn portal() -> Portal {
    Portal {
        identity: Identity {
            greeting: "who a human is",
        },
        organisations: Organisations {
            company: "what a business is",
        },
    }
}

#[test]
fn two_contexts_mount_on_one_router() {
    let router = register_actions(Router::<Portal>::new()).expect("both mount");
    assert_eq!(router.len(), 2);
}

#[tokio::test]
async fn each_handler_receives_the_state_it_was_written_against() {
    let router = register_actions(Router::<Portal>::new()).expect("both mount");
    let server = ApiServer::new(
        ServerConfig::new("127.0.0.1:0".parse().expect("a local address")),
        portal(),
        router,
    );
    let listener = server.serve().await.expect("serve");
    let addr = listener.local_addr().address().expect("a bound address");
    let client = reqwest::Client::new();

    let identity: Answer = client
        .post(format!("http://{addr}/who"))
        .json(&serde_json::json!(null))
        .send()
        .await
        .expect("request")
        .json()
        .await
        .expect("json");
    assert_eq!(identity.said, "who a human is");

    let organisations: Answer = client
        .post(format!("http://{addr}/where"))
        .json(&serde_json::json!(null))
        .send()
        .await
        .expect("request")
        .json()
        .await
        .expect("json");
    assert_eq!(organisations.said, "what a business is");

    listener.stop().await.expect("stop");
}

// The blanket `impl<T: Clone> FromRef<T> for T` is what keeps this compiling.
#[test]
fn a_single_context_surface_projects_to_itself() {
    let router = fundamental_server::Router::<Identity>::new()
        .route(<Who as fundamental_server::ServerAction<Identity>>::route())
        .expect("the identity route mounts on its own state");
    assert_eq!(router.len(), 1);
}
