//! An event has one effect: delivered once, published twice, replayed after a consumer reset,
//! or raced by two replicas at once. Requires Docker.

mod support;

use std::time::Duration;

use fundamental_db::IsolationLevel;
use fundamental_inbox::{Binding, EffectTransaction, Inbound, Inbox, StepFuture};
use fundamental_nats::DeliverPolicy;
use support::BookingError;
use support::{
    DepositPaid, book_step, count, eventually, inbox, nats, publish_twice, schema_with_inbox,
    stream,
};
use tracing_test::traced_test;

const WITHIN: Duration = Duration::from_secs(20);

#[tokio::test]
async fn an_event_is_applied_once_and_recorded() {
    let schema = schema_with_inbox().await;
    let pool = schema.pool();
    let js = nats().await.jetstream();
    let stream = stream(&js, "applied").await;
    let inbox = inbox(pool).await;
    let listener = inbox
        .listen(
            &js,
            support::binding("billing.applied", stream, "applied"),
            book_step,
        )
        .await
        .expect("listening");

    let event = DepositPaid::on("applied", 950_000);
    js.publish_event(&event).await.expect("publish");

    assert!(
        eventually(WITHIN, || async {
            count(pool, "SELECT count(*) FROM bookings").await == 1
        })
        .await,
        "the effect never ran"
    );
    let recorded = count(
        pool,
        &format!(
            "SELECT count(*) FROM fundamental_inbox WHERE effect = 'billing.applied' AND event_id = '{}'",
            event.id
        ),
    )
    .await;
    assert_eq!(recorded, 1, "the effect committed without its inbox row");
    listener.drain().await.expect("drain");
}

#[tokio::test]
async fn an_event_published_twice_has_one_effect() {
    let schema = schema_with_inbox().await;
    let pool = schema.pool();
    let js = nats().await.jetstream();
    let stream = stream(&js, "twice").await;
    let inbox = inbox(pool).await;
    let listener = inbox
        .listen(
            &js,
            support::binding("billing.twice", stream, "twice"),
            book_step,
        )
        .await
        .expect("listening");

    publish_twice(&js, &DepositPaid::on("twice", 1)).await;

    assert!(
        eventually(WITHIN, || support::settled(&js, stream, "twice")).await,
        "both copies were not settled"
    );
    assert_eq!(
        count(pool, "SELECT count(*) FROM bookings").await,
        1,
        "the second copy had a second effect"
    );
    listener.drain().await.expect("drain");
}

#[tokio::test]
async fn a_replay_after_a_consumer_reset_has_no_second_effect() {
    let schema = schema_with_inbox().await;
    let pool = schema.pool();
    let js = nats().await.jetstream();
    let stream = stream(&js, "replayed").await;
    let first = inbox(pool).await;
    let listener = first
        .listen(
            &js,
            support::binding("billing.replayed", stream, "replayed"),
            book_step,
        )
        .await
        .expect("listening");
    for cents in 1..=5 {
        js.publish_event(&DepositPaid::on("replayed", cents))
            .await
            .expect("publish");
    }
    assert!(
        eventually(WITHIN, || async {
            count(pool, "SELECT count(*) FROM bookings").await == 5
        })
        .await,
        "the first pass did not apply all five"
    );
    listener.drain().await.expect("drain");

    // The operator deletes the consumer and creates it again from the start of the stream,
    // under a new name: every stored event is delivered again.
    support::consumer(&js, stream, "replayed-again", DeliverPolicy::All).await;
    let second = inbox(pool).await;
    let listener = second
        .listen(
            &js,
            support::binding("billing.replayed", stream, "replayed-again"),
            book_step,
        )
        .await
        .expect("listening again");
    assert!(
        eventually(WITHIN, || support::settled(&js, stream, "replayed-again")).await,
        "the replay was not settled"
    );
    assert_eq!(
        count(pool, "SELECT count(*) FROM bookings").await,
        5,
        "the replay had second effects"
    );
    listener.drain().await.expect("drain");
}

/// Books the deposit, holding the transaction open for half a second — longer than the gap
/// between an event's two stored copies, so the second copy's insert meets the first's
/// uncommitted row on the other replica and waits for it.
async fn book_slowly(
    tx: &mut EffectTransaction<'_>,
    event: &Inbound<DepositPaid>,
) -> Result<(), BookingError> {
    support::book(tx, event).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    Ok(())
}

fn book_slowly_step<'a, 't>(
    tx: &'a mut EffectTransaction<'t>,
    event: &'a Inbound<DepositPaid>,
) -> StepFuture<'a, BookingError> {
    Box::pin(book_slowly(tx, event))
}

/// Fifty events, each stored twice, drained by two replicas of one consumer at once; a raced
/// copy shows as a duplicate whose insert waited on the other replica's uncommitted row.
#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "readability baseline"
)]
async fn two_replicas_race(level: IsolationLevel, lane: &'static str, effect: &'static str) {
    let schema = schema_with_inbox().await;
    let pool = schema.pool();
    let js = nats().await.jetstream();
    let stream = stream(&js, lane).await;
    let binding = Binding::new(effect, support::HANDLER, stream, lane, level).expect("a binding");
    let replicas = [inbox(pool).await, inbox(pool).await];
    let mut listeners = Vec::new();
    for replica in &replicas {
        listeners.push(
            replica
                .listen(&js, binding, book_slowly_step)
                .await
                .expect("listening"),
        );
    }
    let events: Vec<DepositPaid> = (0..50).map(|cents| DepositPaid::on(lane, cents)).collect();
    for event in &events {
        js.publish_event(event).await.expect("publish");
    }
    tokio::time::sleep(Duration::from_millis(150)).await;
    for event in &events {
        let again = js.publish_event(event).await.expect("publish again");
        assert!(!again.duplicate);
    }

    assert!(
        eventually(Duration::from_secs(60), || support::settled(
            &js, stream, lane
        ))
        .await,
        "not every copy was settled at {level:?}"
    );
    assert_eq!(
        count(pool, "SELECT count(*) FROM bookings").await,
        50,
        "a raced event had a second effect at {level:?}"
    );
    assert_eq!(
        count(pool, "SELECT count(DISTINCT event_id) FROM bookings").await,
        50,
        "an event had no effect at {level:?}"
    );
    // The dispatch loop runs on its own task, outside this test's span, so its lines are read
    // from the whole capture — this process runs this test alone — by this test's effect.
    let captured = String::from_utf8_lossy(
        &tracing_test::internal::global_buf()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
    .into_owned();
    let waited = captured
        .lines()
        .filter(|line| line.contains(effect) && line.contains("inbox.duplicate"))
        .filter_map(|line| line.split("elapsed_ms=").nth(1))
        .filter_map(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
        .filter_map(|ms| ms.parse::<u64>().ok())
        .filter(|ms| *ms >= 150)
        .count();
    assert!(
        waited > 0,
        "no duplicate waited on an uncommitted copy at {level:?}: the replicas never raced"
    );
    for listener in listeners {
        listener.drain().await.expect("drain");
    }
    drop::<[Inbox; 2]>(replicas);
}

#[tokio::test]
#[traced_test]
async fn the_same_event_on_two_replicas_at_once_has_one_effect_serializable() {
    two_replicas_race(
        IsolationLevel::Serializable,
        "race-serializable",
        "billing.race-s",
    )
    .await;
}

#[tokio::test]
#[traced_test]
async fn the_same_event_on_two_replicas_at_once_has_one_effect_repeatable_read() {
    two_replicas_race(
        IsolationLevel::RepeatableRead,
        "race-repeatable",
        "billing.race-r",
    )
    .await;
}

#[tokio::test]
#[traced_test]
async fn the_same_event_on_two_replicas_at_once_has_one_effect_read_committed() {
    two_replicas_race(
        IsolationLevel::ReadCommitted,
        "race-read-committed",
        "billing.race-c",
    )
    .await;
}
