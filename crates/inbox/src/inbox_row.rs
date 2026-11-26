//! [`InboxRow`] — the record that an effect applied an event.

use chrono::{DateTime, Utc};
use fundamental_macros::Model;
use uuid::Uuid;

/// One event an effect applied, written in the same transaction as the effect — so a row
/// present means the effect committed.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(
    sql,
    table = "fundamental_inbox",
    index(name = "fundamental_inbox_by_age", columns = ["effect hash", "sweep_bucket hash", "processed_at asc"]),
    check(name = "fundamental_inbox_effect_length", expr = "char_length(effect) BETWEEN 1 AND 100"),
    check(name = "fundamental_inbox_bucket_range", expr = "sweep_bucket BETWEEN 0 AND 63"),
    check(name = "fundamental_inbox_digest_length", expr = "octet_length(payload_digest) = 32")
)]
pub(crate) struct InboxRow {
    /// The effect's declared name, leading the key.
    #[model(id, shard = hash)]
    pub(crate) effect: String,
    /// The event's own id.
    #[model(id, shard = hash)]
    pub(crate) event_id: Uuid,
    /// Which of the sixty-four age ranges the row is swept from.
    pub(crate) sweep_bucket: i32,
    /// The digest of the payload that was applied.
    pub(crate) payload_digest: Vec<u8>,
    /// The stream that delivered it.
    pub(crate) stream: String,
    /// The consumer that delivered it.
    pub(crate) consumer: String,
    /// Its sequence in that stream.
    pub(crate) stream_sequence: i64,
    /// When the effect committed, by the database's clock.
    pub(crate) processed_at: DateTime<Utc>,
}
