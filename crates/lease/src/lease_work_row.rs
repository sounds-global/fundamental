use chrono::{DateTime, Utc};
use fundamental_macros::Model;

/// One committed piece of fenced work, written in the same transaction as the work.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(
    sql,
    table = "fundamental_lease_work",
    index(name = "fundamental_lease_work_by_age", columns = ["lease asc", "done_at asc"]),
    check(name = "fundamental_lease_work_key_length", expr = r#"char_length(work_key) BETWEEN 1 AND 200"#)
)]
pub struct LeaseWorkRow {
    /// The lease the work ran under, leading the key.
    #[model(id, shard = hash)]
    lease: String,
    /// What the work was.
    #[model(id, shard = hash)]
    work_key: String,
    /// The epoch it committed under.
    epoch: i64,
    /// When it committed, by the database's clock.
    done_at: DateTime<Utc>,
}

impl LeaseWorkRow {
    pub(crate) fn new(lease: &str, work_key: &str, epoch: i64, done_at: DateTime<Utc>) -> Self {
        Self {
            lease: lease.to_owned(),
            work_key: work_key.to_owned(),
            epoch,
            done_at,
        }
    }

    /// The epoch the work was committed under.
    #[must_use]
    pub fn epoch(&self) -> i64 {
        self.epoch
    }

    /// When it committed, by the database's clock.
    #[must_use]
    pub fn done_at(&self) -> DateTime<Utc> {
        self.done_at
    }
}
