use chrono::{DateTime, Utc};
use sqlx::Row as _;

use crate::{SqlError, SqlResult};

/// A reading of the cluster's hybrid logical clock: microseconds in the high bits, a logical counter in the low [`LOGICAL_BITS`](Self::LOGICAL_BITS).
///
/// The database's clock, never the process's, so a token orders against the writes that made it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClusterTime {
    micros: u64,
    logical: u16,
}

impl ClusterTime {
    /// Bits the engine reserves for the logical counter.
    pub const LOGICAL_BITS: u32 = 12;

    /// The statement that reads it.
    const SQL: &'static str = "SELECT yb_get_current_hybrid_time_lsn()";

    /// Reads the clock of the node serving `pool`.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError`] if the statement fails or the value cannot be decoded.
    pub async fn read(pool: &crate::SqlPool) -> SqlResult<Self> {
        Self::decode(&pool.fetch_one(Self::SQL).await?)
    }

    /// Reads the clock as an open transaction's first statement, naming the moment it reads at.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError`] if the statement fails or the value cannot be decoded.
    pub async fn read_in(tx: &mut crate::SqlTransaction) -> SqlResult<Self> {
        Self::decode(&tx.fetch_one(Self::SQL).await?)
    }

    fn decode(row: &sqlx::postgres::PgRow) -> SqlResult<Self> {
        let raw: i64 = row
            .try_get(0)
            .map_err(|e| SqlError::RowConversion(format!("hybrid time: {e}")))?;
        Ok(Self::from_raw(raw))
    }

    /// Decodes a raw hybrid time, reinterpreting the bits, since the unsigned value passes `i64::MAX` around 2041.
    #[must_use]
    #[expect(
        clippy::allow_attributes,
        clippy::allow_attributes_without_reason,
        reason = "readability baseline"
    )]
    pub fn from_raw(raw: i64) -> Self {
        #[allow(clippy::cast_sign_loss)]
        let bits = raw as u64;
        Self {
            micros: bits >> Self::LOGICAL_BITS,
            logical: u16::try_from(bits & ((1 << Self::LOGICAL_BITS) - 1)).unwrap_or(u16::MAX),
        }
    }

    /// Microseconds since the Unix epoch.
    #[must_use]
    pub fn micros(&self) -> u64 {
        self.micros
    }

    /// Nanoseconds since the Unix epoch.
    #[must_use]
    pub fn nanos(&self) -> u64 {
        self.micros.saturating_mul(1_000)
    }

    /// The logical counter that orders readings within one microsecond.
    #[must_use]
    pub fn logical(&self) -> u16 {
        self.logical
    }

    /// This reading as a UTC timestamp at microsecond resolution, without the logical counter.
    #[must_use]
    pub fn to_utc(&self) -> DateTime<Utc> {
        i64::try_from(self.micros)
            .ok()
            .and_then(DateTime::from_timestamp_micros)
            .unwrap_or(DateTime::<Utc>::MAX_UTC)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_engines_encoding() {
        let ct = ClusterTime::from_raw(7_319_054_467_406_954_496);
        assert_eq!(ct.micros(), 1_786_878_532_081_776);
        assert_eq!(ct.nanos(), 1_786_878_532_081_776_000);
    }

    #[test]
    fn to_utc_round_trips_the_engines_encoding() {
        let ct = ClusterTime::from_raw(7_319_054_467_406_954_496);
        let at = ct.to_utc();
        assert_eq!(u64::try_from(at.timestamp_micros()).ok(), Some(ct.micros()));
    }

    #[test]
    fn the_largest_reading_converts_exactly() {
        let ct = ClusterTime::from_raw(-1);
        let at = ct.to_utc();
        assert_ne!(at, DateTime::<Utc>::MAX_UTC);
        assert_eq!(u64::try_from(at.timestamp_micros()).ok(), Some(ct.micros()));
    }

    #[test]
    fn ordering_survives_the_sign_bit() {
        // Either side of `i64::MAX`, which the engine reaches around 2041.
        let earlier = ClusterTime::from_raw(i64::MAX - 1);
        let later = ClusterTime::from_raw(i64::MIN + 1);
        assert!(
            later > earlier,
            "a reading past i64::MAX must not order before one under it: \
             {later:?} vs {earlier:?}"
        );
        assert!(later.micros() > earlier.micros());
    }

    #[test]
    fn the_logical_counter_orders_within_one_microsecond() {
        let first = ClusterTime::from_raw((1_000 << ClusterTime::LOGICAL_BITS) | 1);
        let second = ClusterTime::from_raw((1_000 << ClusterTime::LOGICAL_BITS) | 2);
        assert_eq!(first.micros(), second.micros());
        assert!(second > first);
    }
}
