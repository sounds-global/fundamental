//! Rows past the retention window are removed by a bounded sweep, logged with counts and never
//! an id; rows inside the window stay. Requires Docker.

mod support;

use support::{book_step, count, inbox, nats, schema_with_inbox, stream};
use tracing_test::traced_test;
use uuid::Uuid;

/// Inserts `rows` inbox rows for `effect` in `bucket`, processed `hours_ago`.
async fn aged_rows(
    pool: &fundamental_sql::SqlPool,
    effect: &str,
    bucket: i32,
    rows: i64,
    hours_ago: i64,
) {
    pool.execute(&format!(
        "INSERT INTO fundamental_inbox \
         (effect, event_id, sweep_bucket, payload_digest, stream, consumer, stream_sequence, processed_at) \
         SELECT '{effect}', gen_random_uuid(), {bucket}, decode(repeat('00', 32), 'hex'), 'S', 'C', n, \
         now() - interval '{hours_ago} hours' FROM generate_series(1, {rows}) AS n"
    ))
    .await
    .expect("aged rows");
}

/// Ages `effect`'s published watermark past the grace period, as if every replica had had the
/// time to learn it.
async fn stood_its_grace(pool: &fundamental_sql::SqlPool, effect: &str) {
    pool.execute(&format!(
        "UPDATE fundamental_inbox_effects \
         SET covered_from_set_at = covered_from_set_at - interval '3 minutes' WHERE effect = '{effect}'"
    ))
    .await
    .expect("the watermark aged");
}

#[tokio::test]
#[traced_test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn the_sweep_deletes_at_most_one_batch_per_statement_and_logs_counts_only() {
    let schema = schema_with_inbox().await;
    let pool = schema.pool();
    let js = nats().await.jetstream();
    let stream = stream(&js, "swept").await;
    let inbox = inbox(pool).await;
    let listener = inbox
        .listen(
            &js,
            support::binding("billing.swept", stream, "swept"),
            book_step,
        )
        .await
        .expect("listening");
    aged_rows(pool, "billing.swept", 5, 2_500, 200).await;
    aged_rows(pool, "billing.swept", 6, 10, 100).await;
    let marker = Uuid::now_v7();
    pool.execute(&format!(
        "INSERT INTO fundamental_inbox \
         (effect, event_id, sweep_bucket, payload_digest, stream, consumer, stream_sequence, processed_at) \
         VALUES ('billing.swept', '{marker}', 7, decode(repeat('00', 32), 'hex'), 'S', 'C', 1, now() - interval '300 hours')"
    ))
    .await
    .expect("the marked row");

    // The first pass publishes a watermark and deletes nothing below it: a replica may still
    // be relying on an older reading.
    let first = inbox.sweep().await.expect("the first sweep");
    assert_eq!(
        first.deleted, 0,
        "a pass deleted below a watermark it had just published"
    );
    assert_eq!(
        count(
            pool,
            "SELECT count(*) FROM fundamental_inbox WHERE effect = 'billing.swept'"
        )
        .await,
        2_511
    );
    stood_its_grace(pool, "billing.swept").await;

    let report = inbox.sweep().await.expect("the sweep");
    assert_eq!(report.deleted, 2_501);
    assert_eq!(report.effects, 1);
    assert!(!report.behind);
    // Bucket 5 takes three statements (1000, 1000, 500); each of the other 63 one that finds
    // fewer than a batch; the parked records one more.
    assert_eq!(
        report.batches,
        3 + 63 + 1,
        "a statement deleted more than one batch"
    );
    assert_eq!(
        count(
            pool,
            "SELECT count(*) FROM fundamental_inbox WHERE effect = 'billing.swept'"
        )
        .await,
        10,
        "a row inside the window was swept"
    );
    assert!(logs_contain("inbox.swept"));
    assert!(logs_contain("deleted=2501"));
    let marker = marker.to_string();
    logs_assert(|lines: &[&str]| {
        let sweep: Vec<&&str> = lines
            .iter()
            .filter(|line| line.contains("inbox."))
            .collect();
        match sweep.iter().find(|line| line.contains(&marker)) {
            Some(line) => Err(format!("an inbox line carried an event id: {line}")),
            None if sweep.is_empty() => Err("the sweep logged nothing".to_owned()),
            None => Ok(()),
        }
    });
    listener.drain().await.expect("drain");
}

#[tokio::test]
async fn the_sweep_removes_parked_records_past_their_window_only() {
    let schema = schema_with_inbox().await;
    let pool = schema.pool();
    let js = nats().await.jetstream();
    let stream = stream(&js, "parked-swept").await;
    let inbox = inbox(pool).await;
    let listener = inbox
        .listen(
            &js,
            support::binding("billing.parked-swept", stream, "parked-swept"),
            book_step,
        )
        .await
        .expect("listening");
    for (sequence, hours_ago) in [(1, 800), (2, 10)] {
        pool.execute(&format!(
            "INSERT INTO fundamental_inbox_parked \
             (effect, stream, stream_created_at, stream_sequence, reason, delivered, parked_at) \
             VALUES ('billing.parked-swept', 'S', now(), {sequence}, 'undecodable', 1, now() - interval '{hours_ago} hours')"
        ))
        .await
        .expect("a parked record");
    }
    let report = inbox.sweep().await.expect("the sweep");
    assert_eq!(report.parked_deleted, 1);
    assert_eq!(
        count(
            pool,
            "SELECT count(*) FROM fundamental_inbox_parked WHERE stream_sequence = 2"
        )
        .await,
        1,
        "a parked record inside its window was swept"
    );
    listener.drain().await.expect("drain");
}

#[tokio::test]
async fn an_effect_nothing_listens_for_is_left_alone() {
    let schema = schema_with_inbox().await;
    let pool = schema.pool();
    aged_rows(pool, "billing.elsewhere", 1, 5, 200).await;
    let report = inbox(pool).await.sweep().await.expect("the sweep");
    assert_eq!(report.effects, 0);
    assert_eq!(
        count(pool, "SELECT count(*) FROM fundamental_inbox").await,
        5
    );
}
