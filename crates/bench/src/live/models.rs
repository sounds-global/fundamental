use fundamental_jobs::JobKind;
use fundamental_macros::{Model, Variant};

/// One numbered write.
#[derive(Clone, Debug, Model)]
#[model(sql, table = "bench_notes")]
pub struct Note {
    #[model(id)]
    pub n: i64,
    pub owner_id: String,
    pub body: String,
}

/// A note as a device syncs it: the same table, through its sync overlay.
#[derive(Clone, Debug, Model)]
#[model(offline, syncable, table = "bench_notes")]
pub struct SyncedNote {
    #[model(id)]
    pub n: String,
    pub owner_id: String,
    pub body: String,
}

/// One job fired, written in the fire's own transaction.
#[derive(Clone, Debug, Model)]
#[model(sql, table = "bench_fired")]
pub struct Fired {
    #[model(id)]
    pub subject: String,
}

/// The app's one kind of job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Variant)]
pub enum BenchJob {
    /// A reminder about a note.
    Reminder,
}

impl JobKind for BenchJob {}
