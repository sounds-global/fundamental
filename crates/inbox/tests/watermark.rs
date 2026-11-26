//! An event older than its effect's watermark is never applied, whatever retention the
//! listener is configured with: rows below the watermark may already be swept. Requires Docker.

mod support;

use std::time::Duration;

use fundamental_nats::EventId;
use support::{
    DepositPaid, book_step, count, eventually, inbox, nats, schema_with_inbox, settled, stream,
};
use uuid::{NoContext, Timestamp, Uuid};

const WITHIN: Duration = Duration::from_secs(20);

#[tokio::test]
async fn raising_retention_after_a_sweep_does_not_reapply_swept_events() {
    let schema = schema_with_inbox().await;
    let pool = schema.pool();
    let js = nats().await.jetstream();
    let stream = stream(&js, "watermarked").await;
    let listener = inbox(pool)
        .await
        .listen(
            &js,
            support::binding("billing.watermarked", stream, "watermarked"),
            book_step,
        )
        .await
        .expect("listening");
    // A sweep configured for a day has run: rows processed before then may be gone. This
    // listener is configured for a week, and its own window would accept a two-day-old event.
    pool.execute(
        "UPDATE fundamental_inbox_effects SET \
         covered_from = now() - interval '24 hours', \
         covered_from_set_at = now() - interval '10 minutes', \
         delete_below = now() - interval '24 hours' \
         WHERE effect = 'billing.watermarked'",
    )
    .await
    .expect("a watermark");

    let two_days_ago = u64::try_from(chrono::Utc::now().timestamp() - 48 * 3_600).expect("a time");
    let old = DepositPaid {
        id: EventId::from_uuid(Uuid::new_v7(Timestamp::from_unix(
            NoContext,
            two_days_ago,
            0,
        ))),
        lane: "watermarked".to_owned(),
        cents: 1,
    };
    js.publish_event(&old).await.expect("publish the old event");
    js.publish_event(&DepositPaid::on("watermarked", 2))
        .await
        .expect("publish a new one");

    assert!(
        eventually(WITHIN, || settled(&js, stream, "watermarked")).await,
        "never settled"
    );
    assert_eq!(
        count(pool, "SELECT count(*) FROM bookings WHERE cents = 1").await,
        0,
        "an event below the watermark was applied"
    );
    assert_eq!(
        count(pool, "SELECT count(*) FROM bookings WHERE cents = 2").await,
        1
    );
    assert_eq!(
        count(
            pool,
            "SELECT count(*) FROM fundamental_inbox_parked WHERE reason = 'beyond_horizon'"
        )
        .await,
        1
    );
    listener.drain().await.expect("drain");
}
