//! An operator sees and resolves what is parked through `inbox:parked` and `inbox:resolve`,
//! without reading a table. Requires Docker.

mod support;

use std::time::Duration;

use fundamental_command::{CommandArgs, CommandRunner};
use fundamental_core::Error as _;
use fundamental_inbox::{
    Inbox, InboxContext, InboxError, ParkedAddress, parked_command, rehome_command, resolve_command,
};
use support::{count, eventually, inbox, nats, schema_with_inbox, settled, stream, subject};

struct App {
    inbox: Inbox,
}

impl InboxContext for App {
    fn inbox(&self) -> &Inbox {
        &self.inbox
    }
}

fn runner() -> CommandRunner<App> {
    CommandRunner::new()
        .register(parked_command())
        .register(resolve_command())
        .register(rehome_command())
}

fn args(values: &[&str]) -> CommandArgs {
    CommandArgs::new(values.iter().map(|value| (*value).to_owned()))
}

#[tokio::test]
async fn parked_lists_every_record_and_resolve_removes_exactly_one() {
    let schema = schema_with_inbox().await;
    let pool = schema.pool();
    let js = nats().await.jetstream();
    let stream = stream(&js, "operated").await;
    let app = App {
        inbox: inbox(pool).await,
    };
    let listener = app
        .inbox
        .listen(
            &js,
            support::binding("billing.operated", stream, "operated"),
            support::book_step,
        )
        .await
        .expect("listening");
    for _ in 0..2 {
        js.publish_bytes(&subject("operated"), "not json".into())
            .await
            .expect("publish");
    }
    assert!(eventually(Duration::from_secs(20), || settled(&js, stream, "operated")).await);

    let parked = app
        .inbox
        .parked("billing.operated")
        .await
        .expect("the listing");
    assert_eq!(parked.len(), 2);
    assert!(
        parked
            .iter()
            .all(|record| record.address.effect == "billing.operated")
    );
    runner()
        .run(
            "inbox:parked",
            &app,
            args(&["--effect", "billing.operated"]),
        )
        .await
        .expect("the command lists them");

    let address = parked[0].address.to_string();
    runner()
        .run("inbox:resolve", &app, args(&[&address]))
        .await
        .expect("the command resolves one");
    assert_eq!(
        count(pool, "SELECT count(*) FROM fundamental_inbox_parked").await,
        1,
        "resolve removed other than exactly one"
    );
    let remaining = app
        .inbox
        .parked("billing.operated")
        .await
        .expect("the listing");
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].address, parked[1].address);
    listener.drain().await.expect("drain");
}

#[tokio::test]
async fn resolving_what_is_not_parked_is_refused() {
    let schema = schema_with_inbox().await;
    let app = App {
        inbox: inbox(schema.pool()).await,
    };
    let address: ParkedAddress = "billing.none/S/2026-09-30T12:00:00Z/1"
        .parse()
        .expect("an address");
    let error = app
        .inbox
        .resolve(&address)
        .await
        .expect_err("nothing there");
    assert!(matches!(error, InboxError::ParkedNotFound { .. }));
    assert_eq!(error.error_code(), "SINBOX_013");
    let refused = runner()
        .run("inbox:resolve", &app, args(&["not-an-address"]))
        .await
        .expect_err("a malformed address");
    assert!(refused.to_string().contains("SINBOX_014"), "{refused}");
    let refused = runner()
        .run("inbox:parked", &app, args(&[]))
        .await
        .expect_err("no effect named");
    assert!(refused.to_string().contains("--effect"), "{refused}");
}

#[tokio::test]
async fn rehome_hands_an_effect_to_the_handler_named() {
    let schema = schema_with_inbox().await;
    let pool = schema.pool();
    let js = nats().await.jetstream();
    let stream = stream(&js, "rehomed").await;
    let app = App {
        inbox: inbox(pool).await,
    };
    app.inbox
        .listen(
            &js,
            support::binding("billing.rehomed", stream, "rehomed"),
            support::book_step,
        )
        .await
        .expect("listening")
        .drain()
        .await
        .expect("drain");
    runner()
        .run(
            "inbox:rehome",
            &app,
            args(&["billing.rehomed", "billing::moved::book"]),
        )
        .await
        .expect("the command rehomes it");
    assert_eq!(
        count(
            pool,
            "SELECT count(*) FROM fundamental_inbox_effects WHERE handler = 'billing::moved::book'"
        )
        .await,
        1
    );
    let refused = runner()
        .run("inbox:rehome", &app, args(&["billing.rehomed"]))
        .await
        .expect_err("no handler named");
    assert!(refused.to_string().contains("required"), "{refused}");
    let refused = runner()
        .run("inbox:rehome", &app, args(&["billing.nothing", "x"]))
        .await
        .expect_err("no such effect");
    assert!(refused.to_string().contains("SINBOX_016"), "{refused}");
}
