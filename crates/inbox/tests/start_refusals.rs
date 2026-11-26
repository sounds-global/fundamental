//! A listener refuses, before it takes a delivery, a set-up the inbox cannot keep its promise
//! on. Requires Docker.

mod support;

use std::time::Duration;

use fundamental_core::Error as _;
use fundamental_db::IsolationLevel;
use fundamental_inbox::{Binding, Inbox, InboxError};
use fundamental_nats::{AckPolicy, ConsumerConfig, StreamConfig};
use fundamental_test_containers::nats::NatsServer;
use fundamental_test_containers::sql::SqlSchema;
use support::{book_step, inbox, nats, quick, schema_with_inbox, stream, subject};

#[tokio::test]
async fn an_inbox_without_its_tables_is_refused() {
    let schema = SqlSchema::mint().await.expect("a bare schema");
    let error = Inbox::open(schema.pool().clone(), quick())
        .await
        .expect_err("no inbox tables");
    assert!(matches!(error, InboxError::TableMissing), "{error:?}");
    assert_eq!(error.error_code(), "SINBOX_002");
}

#[tokio::test]
async fn a_consumer_that_acknowledges_in_bulk_or_gives_up_is_refused() {
    let schema = schema_with_inbox().await;
    let js = nats().await.jetstream();
    let name = NatsServer::stream_name("UNSAFE").expect("a name");
    js.create_stream(StreamConfig::new(&name).subjects([subject("unsafe")]))
        .await
        .expect("stream");
    js.get_stream(&name)
        .await
        .expect("stream")
        .create_consumer(
            ConsumerConfig::new("unsafe")
                .durable()
                .ack_policy(AckPolicy::All)
                .max_deliver(5)
                .ack_wait(Duration::from_secs(2)),
        )
        .await
        .expect("consumer");
    let stream: &'static str = Box::leak(name.into_boxed_str());
    let error = inbox(schema.pool())
        .await
        .listen(
            &js,
            support::binding("billing.unsafe", stream, "unsafe"),
            book_step,
        )
        .await
        .expect_err("an unsafe consumer");
    assert_eq!(error.error_code(), "SINBOX_004");
    let InboxError::UnsafeConsumer { problems, .. } = error else {
        panic!("not UnsafeConsumer: {error:?}");
    };
    assert!(problems.contains("ack policy"), "{problems}");
    assert!(
        problems.contains("max_deliver"),
        "both problems are reported: {problems}"
    );
}

#[tokio::test]
async fn one_effect_declared_twice_is_refused() {
    let schema = schema_with_inbox().await;
    let js = nats().await.jetstream();
    let stream = stream(&js, "twice-declared").await;
    let inbox = inbox(schema.pool()).await;
    let binding = support::binding("billing.twice-declared", stream, "twice-declared");
    let listener = inbox
        .listen(&js, binding, book_step)
        .await
        .expect("the first");
    let error = inbox
        .listen(&js, binding, book_step)
        .await
        .expect_err("the second");
    assert_eq!(error.error_code(), "SINBOX_005");
    listener.drain().await.expect("drain");
}

#[tokio::test]
async fn a_missing_stream_is_a_broker_error_and_frees_the_effect() {
    let schema = schema_with_inbox().await;
    let js = nats().await.jetstream();
    let inbox = inbox(schema.pool()).await;
    let binding = support::binding("billing.nowhere", "NO_SUCH_STREAM", "nowhere");
    let error = inbox
        .listen(&js, binding, book_step)
        .await
        .expect_err("no stream");
    assert_eq!(error.error_code(), "SINBOX_012");
    let again = inbox
        .listen(&js, binding, book_step)
        .await
        .expect_err("still no stream");
    assert_eq!(
        again.error_code(),
        "SINBOX_012",
        "a failed start kept the effect declared"
    );
}

#[tokio::test]
async fn an_effect_claimed_by_another_handler_is_refused_until_rehomed() {
    let schema = schema_with_inbox().await;
    let js = nats().await.jetstream();
    let stream = stream(&js, "claimed").await;
    let first = inbox(schema.pool()).await;
    let listener = first
        .listen(
            &js,
            support::binding("billing.claimed", stream, "claimed"),
            book_step,
        )
        .await
        .expect("the first handler claims it");
    listener.drain().await.expect("drain");

    // Another service, another handler, the same name: its events would read as applied.
    let second = inbox(schema.pool()).await;
    let moved = Binding::new(
        "billing.claimed",
        "billing::moved::book",
        stream,
        "claimed",
        IsolationLevel::Serializable,
    )
    .expect("a binding");
    let error = second
        .listen(&js, moved, book_step)
        .await
        .expect_err("claimed elsewhere");
    assert_eq!(error.error_code(), "SINBOX_005");
    let InboxError::EffectDeclaredTwice { detail, .. } = &error else {
        panic!("not EffectDeclaredTwice: {error:?}");
    };
    assert!(detail.contains(support::HANDLER), "{detail}");
    assert!(
        detail.contains("inbox:rehome"),
        "the refusal must say what to do: {detail}"
    );

    second
        .rehome("billing.claimed", "billing::moved::book")
        .await
        .expect("rehomed");
    let listener = second
        .listen(&js, moved, book_step)
        .await
        .expect("the new handler holds it");
    listener.drain().await.expect("drain");

    let error = second
        .rehome("billing.nothing", "x")
        .await
        .expect_err("no such effect");
    assert_eq!(error.error_code(), "SINBOX_016");
}
