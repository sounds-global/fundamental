//! Connection pool status reporting.

/// The current status of a connection pool.
///
/// A field a backend cannot measure is `None` rather than a zero that reads as measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolStatus {
    /// Connections currently in use, where the backend can count them.
    pub active: Option<u32>,
    /// Idle connections available, where the backend can count them.
    pub idle: Option<u32>,
    /// The ceiling on connections this pool may hold.
    pub max: u32,
    /// The floor the backend keeps open, where it has such a concept.
    pub min: Option<u32>,
    /// Callers waiting for a connection right now, where the backend can count them: the
    /// saturation signal.
    pub pending: Option<u32>,
}

impl PoolStatus {
    /// Total connections held — active plus idle — or `None` when either is uncountable.
    #[must_use]
    pub fn total(&self) -> Option<u32> {
        Some(self.active?.saturating_add(self.idle?))
    }

    /// The percentage of the ceiling currently in use, or `None` when nothing counted it.
    ///
    /// A zero ceiling yields zero rather than a division.
    #[must_use]
    pub fn utilization(&self) -> Option<f32> {
        let active = self.active?;
        if self.max == 0 {
            Some(0.0)
        } else {
            Some((active as f32 / self.max as f32) * 100.0)
        }
    }

    /// Whether every connection the pool may hold is in use, or `None` when uncountable.
    #[must_use]
    pub fn is_saturated(&self) -> Option<bool> {
        Some(self.active? >= self.max)
    }

    /// Whether a caller could expect a connection without waiting, or `None` when neither
    /// half of the question can be answered; an uncountable idle count is not read as zero.
    #[must_use]
    pub fn has_capacity(&self) -> Option<bool> {
        if self.idle.is_some_and(|idle| idle > 0) {
            return Some(true);
        }
        match (self.active, self.idle) {
            (Some(active), _) => Some(active < self.max),
            (None, Some(_)) => None,
            (None, None) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The SQL wire: counts everything.
    fn measured() -> PoolStatus {
        PoolStatus {
            active: Some(5),
            idle: Some(5),
            max: 10,
            min: Some(1),
            pending: Some(0),
        }
    }

    /// The CQL wire: counts nothing.
    fn unmeasured() -> PoolStatus {
        PoolStatus {
            active: None,
            idle: None,
            max: 10,
            min: None,
            pending: None,
        }
    }

    #[test]
    fn a_measured_status_reports_every_derivation() {
        let status = measured();
        assert_eq!(status.total(), Some(10));
        assert_eq!(status.utilization(), Some(50.0));
        assert_eq!(status.is_saturated(), Some(false));
        assert_eq!(status.has_capacity(), Some(true));
    }

    /// An unmeasured field must never read as a measured zero, in any derivation.
    #[test]
    fn an_unmeasured_field_never_derives_a_number() {
        let status = unmeasured();
        assert_eq!(status.total(), None);
        assert_eq!(status.utilization(), None);
        assert_eq!(status.is_saturated(), None);
        assert_eq!(status.has_capacity(), None);
    }

    #[test]
    fn capacity_is_answered_from_whichever_half_can_answer_it() {
        let idle_only = PoolStatus {
            active: None,
            idle: Some(2),
            max: 10,
            min: None,
            pending: None,
        };
        assert_eq!(idle_only.has_capacity(), Some(true));
        assert_eq!(idle_only.total(), None, "a total still needs both");
    }

    #[test]
    fn a_full_pool_is_saturated_and_has_no_capacity() {
        let full = PoolStatus {
            active: Some(10),
            idle: Some(0),
            max: 10,
            min: Some(1),
            pending: Some(3),
        };
        assert_eq!(full.is_saturated(), Some(true));
        assert_eq!(full.has_capacity(), Some(false));
        assert_eq!(full.utilization(), Some(100.0));
    }

    #[test]
    fn a_zero_ceiling_reports_no_utilization_rather_than_dividing_by_it() {
        let unconfigured = PoolStatus {
            active: Some(0),
            idle: Some(0),
            max: 0,
            min: None,
            pending: Some(0),
        };
        assert_eq!(unconfigured.utilization(), Some(0.0));
    }
}
