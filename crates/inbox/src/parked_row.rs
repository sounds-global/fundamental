//! [`ParkedRow`] — the record of a delivery the inbox refused to apply.

use chrono::{DateTime, Utc};
use fundamental_macros::Model;
use uuid::Uuid;

/// One refused delivery, kept for an operator.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(
    sql,
    table = "fundamental_inbox_parked",
    index(name = "fundamental_inbox_parked_by_age", columns = ["effect hash", "parked_at asc"]),
    check(name = "fundamental_inbox_parked_reason", expr = "reason IN ('undecodable', 'invalid_event_id', 'event_id_reused', 'beyond_horizon', 'from_the_future', 'attempts_exhausted', 'handler_panicked')")
)]
pub(crate) struct ParkedRow {
    /// The effect it was delivered to.
    #[model(id, shard = hash)]
    pub(crate) effect: String,
    /// The stream it sits in.
    #[model(id, shard = hash)]
    pub(crate) stream: String,
    /// When that stream was created: sequences restart with a new stream.
    #[model(id, shard = hash)]
    pub(crate) stream_created_at: DateTime<Utc>,
    /// Its sequence in that stream.
    #[model(id, shard = hash)]
    pub(crate) stream_sequence: i64,
    /// The event's id, when it had a usable one.
    pub(crate) event_id: Option<Uuid>,
    /// Why it was refused — a [`ParkReason`](crate::ParkReason)'s column value.
    pub(crate) reason: String,
    /// The error code of the last failure, when a failure parked it.
    pub(crate) error_code: Option<String>,
    /// Which attempt parked it.
    pub(crate) delivered: i64,
    /// When, by the database's clock.
    pub(crate) parked_at: DateTime<Utc>,
}
