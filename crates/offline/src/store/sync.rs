//! Local sync scaffolding: a durable queue of pending local changes.
//!
//! Transport and conflict resolution live in `fundamental_sync`.

use tracing::field::Empty;
use tracing::{Span, debug, instrument};

use crate::db::{OfflineDatabase, OfflineDbError, Value};

/// Tracking table for pending local changes.
const SYNC_TABLE: &str = "_fundamental_pending_sync";

/// The kind of local change waiting to be synced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncOperation {
    /// Record was created locally.
    Insert,
    /// Record was modified locally.
    Update,
    /// Record was deleted locally.
    Delete,
}

impl SyncOperation {
    /// Returns the string representation stored in the queue.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Insert => "insert",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }

    /// Parses from the stored string representation.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "insert" => Some(Self::Insert),
            "update" => Some(Self::Update),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }
}

/// A queued local change.
#[derive(Debug, Clone)]
pub struct PendingChange {
    /// Monotonic queue sequence number.
    pub seq: i64,
    /// Table the change belongs to.
    pub table_name: String,
    /// Primary key of the changed record (as text).
    pub record_id: String,
    /// The kind of change.
    pub operation: SyncOperation,
    /// Optional serialized record payload (e.g. JSON).
    pub payload: Option<String>,
    /// When the change was queued (ISO8601 UTC).
    pub queued_at: String,
}

/// Durable FIFO queue of local changes awaiting sync.
#[derive(Debug)]
pub struct SyncQueue<'a> {
    db: &'a OfflineDatabase,
}

impl<'a> SyncQueue<'a> {
    /// Creates the queue accessor, ensuring the backing table exists.
    #[instrument(level = "debug", skip(db), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn new(db: &'a OfflineDatabase) -> Result<Self, OfflineDbError> {
        db.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS {SYNC_TABLE} (\n    \
                 seq INTEGER PRIMARY KEY AUTOINCREMENT,\n    \
                 table_name TEXT NOT NULL,\n    \
                 record_id TEXT NOT NULL,\n    \
                 operation TEXT NOT NULL,\n    \
                 payload TEXT,\n    \
                 queued_at TEXT NOT NULL\n\
             );"
        ))?;
        Ok(Self { db })
    }

    /// Enqueues a local change.
    #[instrument(
        skip(self, record_id, payload),
        fields(table = table_name, op = operation.as_str(), has_payload = payload.is_some()),
        err(Debug)
    )]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn enqueue(
        &self,
        table_name: &str,
        record_id: &str,
        operation: SyncOperation,
        payload: Option<&str>,
    ) -> Result<(), OfflineDbError> {
        self.db.execute(
            &format!(
                "INSERT INTO {SYNC_TABLE} (table_name, record_id, operation, payload, queued_at)\n \
                 VALUES (?1, ?2, ?3, ?4, datetime('now'));"
            ),
            &[
                Value::from(table_name),
                Value::from(record_id),
                Value::from(operation.as_str()),
                payload.map(Value::from).unwrap_or(Value::Null),
            ],
        )?;
        debug!("enqueued local change for sync");
        Ok(())
    }

    /// Returns all pending changes in queue order.
    #[instrument(level = "debug", skip(self), fields(pending = Empty), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn pending(&self) -> Result<Vec<PendingChange>, OfflineDbError> {
        let rows = self.db.query(
            &format!(
                "SELECT seq, table_name, record_id, operation, payload, queued_at\n \
                 FROM {SYNC_TABLE} ORDER BY seq;"
            ),
            &[],
        )?;

        let changes = rows
            .into_iter()
            .map(|row| {
                let text = |i: usize| -> Result<String, OfflineDbError> {
                    row.get(i)
                        .and_then(|v| v.as_text().map(str::to_string))
                        .ok_or_else(|| OfflineDbError::Decode {
                            model: "PendingChange".to_string(),
                            message: format!("missing column {i}"),
                        })
                };

                let operation_str = text(3)?;
                Ok(PendingChange {
                    seq: row.get(0).and_then(Value::as_integer).unwrap_or_default(),
                    table_name: text(1)?,
                    record_id: text(2)?,
                    operation: SyncOperation::parse(&operation_str).ok_or_else(|| {
                        OfflineDbError::Decode {
                            model: "PendingChange".to_string(),
                            message: format!("unknown operation '{operation_str}'"),
                        }
                    })?,
                    payload: row.get(4).and_then(|v| v.as_text().map(str::to_string)),
                    queued_at: text(5)?,
                })
            })
            .collect::<Result<Vec<_>, OfflineDbError>>()?;
        Span::current().record("pending", changes.len());
        Ok(changes)
    }

    /// Removes changes that have been confirmed synced.
    #[instrument(level = "debug", skip(self), fields(requested = seqs.len(), removed = Empty), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn mark_synced(&self, seqs: &[i64]) -> Result<usize, OfflineDbError> {
        let mut removed = 0;
        for seq in seqs {
            removed += self.db.execute(
                &format!("DELETE FROM {SYNC_TABLE} WHERE seq = ?1;"),
                &[Value::Integer(*seq)],
            )?;
        }
        Span::current().record("removed", removed);
        debug!(removed, "marked sync queue entries as synced");
        Ok(removed)
    }

    /// Number of pending changes.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn len(&self) -> Result<usize, OfflineDbError> {
        let row = self
            .db
            .query_one(&format!("SELECT COUNT(*) FROM {SYNC_TABLE};"), &[])?;
        Ok(row.get(0).and_then(Value::as_integer).unwrap_or_default() as usize)
    }

    /// Returns true if there are no pending changes.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn is_empty(&self) -> Result<bool, OfflineDbError> {
        Ok(self.len()? == 0)
    }
}
