//! An effect that fails takes its inbox row with it, so the redelivery is processed — including
//! a body that swallowed a failed statement and returned success. Requires Docker.

mod support;

use std::time::Duration;

use fundamental_db::RepositoryTransaction as _;
use fundamental_inbox::{EffectTransaction, Inbound, StepFuture};
use support::{
    Booking, BookingError, DepositPaid, count, eventually, inbox, nats, schema_with_inbox, settled,
    stream,
};
use uuid::Uuid;

const WITHIN: Duration = Duration::from_secs(20);

/// Books, then fails on the first delivery only.
async fn fail_first(
    tx: &mut EffectTransaction<'_>,
    event: &Inbound<DepositPaid>,
) -> Result<(), BookingError> {
    support::book(tx, event).await?;
    if event.delivery().delivered == 1 {
        return Err(BookingError::Briefly);
    }
    Ok(())
}

fn fail_first_step<'a, 't>(
    tx: &'a mut EffectTransaction<'t>,
    event: &'a Inbound<DepositPaid>,
) -> StepFuture<'a, BookingError> {
    Box::pin(fail_first(tx, event))
}

/// On the first delivery, books, then writes the same booking again and swallows the unique
/// violation — the statement failed, the server aborted the transaction, and the body still
/// says it succeeded.
async fn swallow_first(
    tx: &mut EffectTransaction<'_>,
    event: &Inbound<DepositPaid>,
) -> Result<(), BookingError> {
    if event.delivery().delivered > 1 {
        return support::book(tx, event).await;
    }
    let booking = Booking {
        booking_id: Uuid::now_v7(),
        event_id: event.event_id().as_uuid(),
        cents: event.payload().cents,
    };
    tx.insert(&booking).await?;
    let swallowed = tx.insert(&booking).await;
    assert!(swallowed.is_err(), "the second insert of one key must fail");
    Ok(())
}

fn swallow_first_step<'a, 't>(
    tx: &'a mut EffectTransaction<'t>,
    event: &'a Inbound<DepositPaid>,
) -> StepFuture<'a, BookingError> {
    Box::pin(swallow_first(tx, event))
}

#[tokio::test]
async fn a_failed_effect_leaves_no_row_and_the_redelivery_is_processed() {
    let schema = schema_with_inbox().await;
    let pool = schema.pool();
    let js = nats().await.jetstream();
    let stream = stream(&js, "fails-once").await;
    let inbox = inbox(pool).await;
    let listener = inbox
        .listen(
            &js,
            support::binding("billing.fails-once", stream, "fails-once"),
            fail_first_step,
        )
        .await
        .expect("listening");
    js.publish_event(&DepositPaid::on("fails-once", 7))
        .await
        .expect("publish");

    assert!(
        eventually(WITHIN, || settled(&js, stream, "fails-once")).await,
        "never settled"
    );
    assert_eq!(
        count(pool, "SELECT count(*) FROM bookings").await,
        1,
        "the failed attempt's booking survived its rollback"
    );
    assert_eq!(
        count(pool, "SELECT count(*) FROM fundamental_inbox").await,
        1
    );
    assert_eq!(
        count(pool, "SELECT count(*) FROM fundamental_inbox_parked").await,
        0
    );
    listener.drain().await.expect("drain");
}

#[tokio::test]
async fn a_swallowed_statement_error_does_not_commit() {
    let schema = schema_with_inbox().await;
    let pool = schema.pool();
    let js = nats().await.jetstream();
    let stream = stream(&js, "swallows").await;
    let inbox = inbox(pool).await;
    let listener = inbox
        .listen(
            &js,
            support::binding("billing.swallows", stream, "swallows"),
            swallow_first_step,
        )
        .await
        .expect("listening");
    let event = DepositPaid::on("swallows", 8);
    js.publish_event(&event).await.expect("publish");

    assert!(
        eventually(WITHIN, || settled(&js, stream, "swallows")).await,
        "never settled"
    );
    assert_eq!(
        count(pool, "SELECT count(*) FROM bookings").await,
        1,
        "an aborted attempt was acknowledged as applied, and the event had no effect"
    );
    assert_eq!(
        count(pool, "SELECT count(*) FROM fundamental_inbox").await,
        1
    );
    listener.drain().await.expect("drain");
}
