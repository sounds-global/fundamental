//! An event the inbox cannot vouch for is parked — recorded for an operator and terminated —
//! never applied and never silently dropped; a panicking handler costs one event, not the
//! listener. Requires Docker.

mod support;

use std::time::Duration;

use fundamental_inbox::{EffectTransaction, Inbound, StepFuture};
use fundamental_nats::{EventId, Message as _};
use support::{
    BookingError, DepositPaid, count, eventually, inbox, nats, schema_with_inbox, settled, stream,
    subject,
};
use uuid::{NoContext, Timestamp, Uuid};

const WITHIN: Duration = Duration::from_secs(30);

fn event_at(lane: &str, unix_secs: i64, cents: i64) -> DepositPaid {
    let secs = u64::try_from(unix_secs).expect("after 1970");
    DepositPaid {
        id: EventId::from_uuid(Uuid::new_v7(Timestamp::from_unix(NoContext, secs, 0))),
        lane: lane.to_owned(),
        cents,
    }
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

async fn parked_reason(pool: &fundamental_sql::SqlPool, reason: &str) -> i64 {
    count(
        pool,
        &format!("SELECT count(*) FROM fundamental_inbox_parked WHERE reason = '{reason}'"),
    )
    .await
}

/// A lane with a listener booking deposits, for the refusals that need no special handler.
async fn booking_lane(
    lane: &'static str,
) -> (
    fundamental_test_containers::sql::SqlSchema,
    fundamental_nats::JetStream,
    &'static str,
    fundamental_nats::Listener,
) {
    let schema = schema_with_inbox().await;
    let js = nats().await.jetstream();
    let stream = stream(&js, lane).await;
    let listener = inbox(schema.pool())
        .await
        .listen(
            &js,
            support::binding(lane, stream, lane),
            support::book_step,
        )
        .await
        .expect("listening");
    (schema, js, stream, listener)
}

#[tokio::test]
async fn an_event_beyond_the_window_is_parked() {
    let (schema, js, stream, listener) = booking_lane("too-old").await;
    let old = event_at("too-old", now_secs() - 30 * 24 * 3_600, 1);
    js.publish_event(&old).await.expect("publish");
    assert!(
        eventually(WITHIN, || settled(&js, stream, "too-old")).await,
        "never settled"
    );
    assert_eq!(
        count(schema.pool(), "SELECT count(*) FROM bookings").await,
        0,
        "an event past the window was applied"
    );
    assert_eq!(parked_reason(schema.pool(), "beyond_horizon").await, 1);
    listener.drain().await.expect("drain");
}

#[tokio::test]
async fn an_event_from_the_future_is_parked() {
    let (schema, js, stream, listener) = booking_lane("too-new").await;
    js.publish_event(&event_at("too-new", now_secs() + 24 * 3_600, 1))
        .await
        .expect("publish");
    assert!(
        eventually(WITHIN, || settled(&js, stream, "too-new")).await,
        "never settled"
    );
    assert_eq!(
        count(schema.pool(), "SELECT count(*) FROM bookings").await,
        0
    );
    assert_eq!(parked_reason(schema.pool(), "from_the_future").await, 1);
    listener.drain().await.expect("drain");
}

#[tokio::test]
async fn a_reused_id_is_parked() {
    let (schema, js, stream, listener) = booking_lane("reused").await;
    let first = DepositPaid::on("reused", 100);
    js.publish_event(&first).await.expect("publish");
    assert!(
        eventually(WITHIN, || async {
            count(schema.pool(), "SELECT count(*) FROM bookings").await == 1
        })
        .await
    );
    tokio::time::sleep(Duration::from_millis(250)).await;
    let reused = DepositPaid {
        cents: 999,
        ..first
    };
    js.publish_event(&reused).await.expect("publish the reuse");
    assert!(
        eventually(WITHIN, || async {
            parked_reason(schema.pool(), "event_id_reused").await == 1
        })
        .await,
        "the reuse was not parked"
    );
    assert!(eventually(WITHIN, || settled(&js, stream, "reused")).await);
    assert_eq!(
        count(schema.pool(), "SELECT count(*) FROM bookings").await,
        1,
        "the reused id had an effect"
    );
    listener.drain().await.expect("drain");
}

#[tokio::test]
async fn a_header_id_mismatch_is_parked() {
    let (schema, js, stream, listener) = booking_lane("mismatched").await;
    let event = DepositPaid::on("mismatched", 5);
    js.publish_bytes_with_msg_id(
        &subject("mismatched"),
        event.to_bytes().expect("bytes"),
        &EventId::generate().to_string(),
    )
    .await
    .expect("publish");
    assert!(
        eventually(WITHIN, || settled(&js, stream, "mismatched")).await,
        "never settled"
    );
    assert_eq!(
        count(schema.pool(), "SELECT count(*) FROM bookings").await,
        0
    );
    assert_eq!(parked_reason(schema.pool(), "invalid_event_id").await, 1);
    listener.drain().await.expect("drain");
}

#[tokio::test]
async fn an_undecodable_payload_is_parked() {
    let (schema, js, stream, listener) = booking_lane("garbled").await;
    js.publish_bytes(&subject("garbled"), "not json".into())
        .await
        .expect("publish");
    assert!(
        eventually(WITHIN, || settled(&js, stream, "garbled")).await,
        "never settled"
    );
    assert_eq!(
        count(schema.pool(), "SELECT count(*) FROM fundamental_inbox_parked WHERE reason = 'undecodable' AND event_id IS NULL AND delivered = 3").await,
        1,
        "an undecodable payload was not given its full budget before it was parked"
    );
    listener.drain().await.expect("drain");
}

async fn always_refuse(
    _tx: &mut EffectTransaction<'_>,
    _event: &Inbound<DepositPaid>,
) -> Result<(), BookingError> {
    Err(BookingError::Refused)
}

fn always_refuse_step<'a, 't>(
    tx: &'a mut EffectTransaction<'t>,
    event: &'a Inbound<DepositPaid>,
) -> StepFuture<'a, BookingError> {
    Box::pin(always_refuse(tx, event))
}

#[tokio::test]
async fn attempts_exhausted_are_parked() {
    let schema = schema_with_inbox().await;
    let pool = schema.pool();
    let js = nats().await.jetstream();
    let stream = stream(&js, "exhausted").await;
    let listener = inbox(pool)
        .await
        .listen(
            &js,
            support::binding("billing.exhausted", stream, "exhausted"),
            always_refuse_step,
        )
        .await
        .expect("listening");
    js.publish_event(&DepositPaid::on("exhausted", 1))
        .await
        .expect("publish");
    assert!(
        eventually(WITHIN, || settled(&js, stream, "exhausted")).await,
        "never settled"
    );
    assert_eq!(
        count(pool, "SELECT count(*) FROM fundamental_inbox_parked WHERE reason = 'attempts_exhausted' AND error_code = 'BOOK_002' AND delivered = 3").await,
        1,
        "the event was not parked after exactly the budget of three attempts"
    );
    assert_eq!(
        count(pool, "SELECT count(*) FROM fundamental_inbox").await,
        0
    );
    listener.drain().await.expect("drain");
}

/// Panics on the deposit of 13 cents; books every other.
async fn panics_on_thirteen(
    tx: &mut EffectTransaction<'_>,
    event: &Inbound<DepositPaid>,
) -> Result<(), BookingError> {
    assert!(event.payload().cents != 13, "thirteen is unlucky");
    support::book(tx, event).await
}

fn panics_on_thirteen_step<'a, 't>(
    tx: &'a mut EffectTransaction<'t>,
    event: &'a Inbound<DepositPaid>,
) -> StepFuture<'a, BookingError> {
    Box::pin(panics_on_thirteen(tx, event))
}

#[tokio::test]
async fn a_panicking_handler_is_parked_not_fatal() {
    let schema = schema_with_inbox().await;
    let pool = schema.pool();
    let js = nats().await.jetstream();
    let stream = stream(&js, "panics").await;
    let listener = inbox(pool)
        .await
        .listen(
            &js,
            support::binding("billing.panics", stream, "panics"),
            panics_on_thirteen_step,
        )
        .await
        .expect("listening");
    js.publish_event(&DepositPaid::on("panics", 13))
        .await
        .expect("publish");
    js.publish_event(&DepositPaid::on("panics", 14))
        .await
        .expect("publish");
    assert!(
        eventually(WITHIN, || settled(&js, stream, "panics")).await,
        "never settled"
    );
    assert_eq!(
        count(pool, "SELECT count(*) FROM bookings WHERE cents = 14").await,
        1,
        "the listener died with the panic"
    );
    assert_eq!(parked_reason(pool, "handler_panicked").await, 1);
    listener.drain().await.expect("drain");
}

#[tokio::test]
async fn a_failed_park_is_nakked_not_terminated() {
    let (schema, js, stream, listener) = booking_lane("park-fails").await;
    let pool = schema.pool();
    pool.execute("DROP TABLE fundamental_inbox_parked")
        .await
        .expect("drop");
    js.publish_bytes(&subject("park-fails"), "not json".into())
        .await
        .expect("publish");
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(
        !settled(&js, stream, "park-fails").await,
        "a delivery was ended with no record of it"
    );

    let ddl = include_str!("../migrations/sql/0002_fundamental_inbox_parked.up.sql");
    // Other suites create tables on the same engine at the same time, and a catalog change can
    // meet theirs as a serialization conflict; a DDL statement is simply run again.
    for statement in ddl.split(';').filter(|part| part.contains("CREATE")) {
        let mut attempts = 0;
        while let Err(error) = pool.execute(statement).await {
            attempts += 1;
            assert!(
                error.is_retryable() && attempts < 5,
                "the table again: {error:?}"
            );
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
    assert!(
        eventually(WITHIN, || settled(&js, stream, "park-fails")).await,
        "never settled"
    );
    assert_eq!(
        count(pool, "SELECT count(*) FROM fundamental_inbox_parked WHERE reason = 'undecodable' AND delivered >= 2").await,
        1,
        "the redelivery after the failed park was not parked"
    );
    listener.drain().await.expect("drain");
}
