//! [`Watermark`] — how far back an effect's ledger is complete, as one replica last read it.

use std::time::{Duration, Instant};

use chrono::{DateTime, TimeDelta, Utc};

/// How often a replica re-reads the watermark, by its own clock.
pub(crate) const REFRESH: Duration = Duration::from_secs(10);

/// How long a reading may be relied on, by the database's clock: half of [`GRACE`].
pub(crate) const TRUST: TimeDelta = TimeDelta::seconds(60);

/// How long a new watermark stands before the sweep deletes below it: every replica has, by
/// then, stopped relying on any older reading.
pub(crate) const GRACE: TimeDelta = TimeDelta::seconds(120);

/// One replica's reading of an effect's watermark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Watermark {
    /// An event older than this is not applied; `None` while nothing has been swept.
    pub(crate) covered_from: Option<DateTime<Utc>>,
    /// The database's clock just before the row was read.
    pub(crate) read_at: DateTime<Utc>,
    /// This process's clock just before the row was read, for deciding when to read again.
    pub(crate) read_locally: Instant,
}

impl Watermark {
    /// Whether this reading is too old to read again soon, by this process's clock.
    pub(crate) fn due(&self) -> bool {
        self.read_locally.elapsed() >= REFRESH
    }

    /// Whether this reading may still be relied on at `now`, by the database's clock.
    pub(crate) fn trusted_at(&self, now: DateTime<Utc>) -> bool {
        now.signed_duration_since(self.read_at) < TRUST
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reading_is_trusted_for_less_than_half_the_grace() {
        assert!(
            TRUST * 2 <= GRACE,
            "a reading must expire well inside the grace"
        );
        let read_at = DateTime::from_timestamp(1_800_000_000, 0).expect("a time");
        let reading = Watermark {
            covered_from: None,
            read_at,
            read_locally: Instant::now(),
        };
        assert!(reading.trusted_at(read_at + TimeDelta::seconds(59)));
        assert!(!reading.trusted_at(read_at + TRUST));
        assert!(!reading.due());
    }
}
