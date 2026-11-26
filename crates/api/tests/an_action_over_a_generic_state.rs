//! A domain's action written once over a capability trait, served over every state that has it.

use fundamental_api::{
    AnyError, ApiListener, ApiServer, HttpContext, MiddlewareResult, Next, Router, ServerConfig,
    ServerListener, decode_body, encode_body,
};
use fundamental_macros::{action, middleware};
use fundamental_server::FromRef;
use serde::{Deserialize, Serialize};

trait Greets: Clone + Send + Sync + 'static {
    fn greeting(&self) -> &'static str;
}

#[derive(Serialize, Deserialize)]
struct Answer {
    said: String,
}

#[derive(Serialize, Deserialize)]
struct Whom {
    name: String,
}

#[action(POST, "/greet")]
async fn greet<S: Greets>(ctx: HttpContext<S>, whom: Whom) -> Result<Answer, AnyError> {
    Ok(Answer {
        said: format!("{}, {}", ctx.state().greeting(), whom.name),
    })
}

#[middleware]
async fn shout<S>(ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult {
    let rewritten = match decode_body::<Whom>(&body)? {
        Some(whom) => encode_body(&Whom {
            name: whom.name.to_uppercase(),
        })?,
        None => body,
    };
    next.run(ctx, rewritten).await
}

#[action(POST, "/greet-loudly", shout)]
async fn greet_loudly<S: Greets>(ctx: HttpContext<S>, whom: Whom) -> Result<Answer, AnyError> {
    Ok(Answer {
        said: format!("{}, {}", ctx.state().greeting(), whom.name),
    })
}

#[action(GET, "/greeting")]
async fn greeting<S>(ctx: HttpContext<S>) -> Result<Answer, AnyError>
where
    S: Greets,
{
    Ok(Answer {
        said: ctx.state().greeting().to_owned(),
    })
}

#[derive(Clone)]
struct Clock {
    hour: u8,
}

#[action(GET, "/hour")]
async fn hour(ctx: HttpContext<Clock>) -> Result<Answer, AnyError> {
    Ok(Answer {
        said: ctx.state().hour.to_string(),
    })
}

#[derive(Clone)]
struct Www {
    clock: Clock,
}

impl Greets for Www {
    fn greeting(&self) -> &'static str {
        "hello"
    }
}

impl FromRef<Www> for Clock {
    fn from_ref(www: &Www) -> Self {
        www.clock.clone()
    }
}

#[derive(Clone)]
struct Ops;

impl Greets for Ops {
    fn greeting(&self) -> &'static str {
        "good morning"
    }
}

async fn serve<S: Clone + Send + Sync + 'static>(
    state: S,
    router: Router<S>,
) -> (String, ApiListener) {
    let server = ApiServer::new(
        ServerConfig::new("127.0.0.1:0".parse().expect("a local address")),
        state,
        router,
    );
    let listener = server.serve().await.expect("serve");
    let addr = listener.local_addr().address().expect("a bound address");
    (format!("http://{addr}"), listener)
}

async fn greeted(base: &str, name: &str) -> String {
    posted(base, "/greet", name).await
}

async fn posted(base: &str, path: &str, name: &str) -> String {
    let answer: Answer = reqwest::Client::new()
        .post(format!("{base}{path}"))
        .json(&Whom {
            name: name.to_owned(),
        })
        .send()
        .await
        .expect("request")
        .json()
        .await
        .expect("json");
    answer.said
}

async fn got(base: &str, path: &str) -> String {
    let answer: Answer = reqwest::get(format!("{base}{path}"))
        .await
        .expect("request")
        .json()
        .await
        .expect("json");
    answer.said
}

#[tokio::test]
async fn one_generic_action_is_served_over_two_states_by_their_own_impls() {
    let www = Router::<Www>::new()
        .action::<Greet>()
        .and_then(Router::action::<Greeting>)
        .expect("mounts over the public state");
    let ops = Router::<Ops>::new()
        .action::<Greet>()
        .and_then(Router::action::<Greeting>)
        .expect("mounts over the operations state");
    let (www_base, www_listener) = serve(
        Www {
            clock: Clock { hour: 9 },
        },
        www,
    )
    .await;
    let (ops_base, ops_listener) = serve(Ops, ops).await;

    assert_eq!(greeted(&www_base, "Ada").await, "hello, Ada");
    assert_eq!(greeted(&ops_base, "Ada").await, "good morning, Ada");
    assert_eq!(got(&www_base, "/greeting").await, "hello");
    assert_eq!(got(&ops_base, "/greeting").await, "good morning");

    www_listener.stop().await.expect("stop");
    ops_listener.stop().await.expect("stop");
}

#[tokio::test]
async fn a_concrete_action_is_projected_beside_a_generic_one() {
    let router = Router::<Www>::new()
        .action::<Greet>()
        .and_then(Router::action::<Hour>)
        .expect("both mount");
    let (base, listener) = serve(
        Www {
            clock: Clock { hour: 9 },
        },
        router,
    )
    .await;

    assert_eq!(greeted(&base, "Ada").await, "hello, Ada");
    assert_eq!(got(&base, "/hour").await, "9");

    listener.stop().await.expect("stop");
}

#[tokio::test]
async fn a_generic_action_runs_a_middleware_for_every_state() {
    let router = Router::<Ops>::new()
        .action::<GreetLoudly>()
        .expect("mounts over the operations state");
    let (base, listener) = serve(Ops, router).await;

    assert_eq!(
        posted(&base, "/greet-loudly", "Ada").await,
        "good morning, ADA"
    );

    listener.stop().await.expect("stop");
}

mod listed {
    use super::{Greet, Greeting, Hour, Www};
    use fundamental_api::Router;
    use fundamental_macros::mount_actions;

    mount_actions! { Greet, Greeting, Hour }

    #[test]
    fn a_generic_action_is_listed_beside_a_concrete_one() {
        let router = register_actions(Router::<Www>::new()).expect("all mount");
        assert_eq!(router.len(), 3);
    }
}
