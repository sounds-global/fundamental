//! [`OutboxInventory`] — what one outbox table holds that its relay will not send.

#[cfg(not(feature = "std"))]
use alloc::collections::BTreeMap;
#[cfg(feature = "std")]
use std::collections::BTreeMap;

use super::{HeldCount, SetAsideBucket};

/// What one outbox table holds, within one tenancy, that its relay will not send on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxInventory {
    set_aside: BTreeMap<SetAsideBucket, HeldCount>,
    beyond_policy: HeldCount,
    skipped: HeldCount,
}

impl OutboxInventory {
    /// An inventory from per-bucket counts, the rows beyond the retry policy, and the skipped rows.
    #[must_use]
    pub fn new(
        set_aside: BTreeMap<SetAsideBucket, HeldCount>,
        beyond_policy: HeldCount,
        skipped: HeldCount,
    ) -> Self {
        Self {
            set_aside,
            beyond_policy,
            skipped,
        }
    }

    /// How many rows are set aside under `bucket`.
    #[must_use]
    pub fn set_aside(&self, bucket: SetAsideBucket) -> HeldCount {
        self.set_aside
            .get(&bucket)
            .copied()
            .unwrap_or(HeldCount::Exact(0))
    }

    /// Every bucket with its count, including the empty ones, so a gauge publishes each.
    pub fn buckets(&self) -> impl Iterator<Item = (SetAsideBucket, HeldCount)> + '_ {
        SetAsideBucket::all().map(|bucket| (bucket, self.set_aside(bucket)))
    }

    /// How many unskipped rows are deferred further out than any retry the relay schedules.
    #[must_use]
    pub fn beyond_policy(&self) -> HeldCount {
        self.beyond_policy
    }

    /// How many waiting rows a claim has skipped because its process's keys could not open them.
    ///
    /// A row every replica skips never leaves, and shows here.
    #[must_use]
    pub fn skipped(&self) -> HeldCount {
        self.skipped
    }

    /// Whether the table holds nothing its relay will not send.
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.beyond_policy.is_none_held()
            && self.skipped.is_none_held()
            && self.buckets().all(|(_, held)| held.is_none_held())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::OutboxInventory;
    use crate::outbox::{HeldCount, RefusalKind, SetAsideBucket};

    #[test]
    fn an_absent_bucket_holds_nothing_and_is_still_listed() {
        let inventory =
            OutboxInventory::new(BTreeMap::new(), HeldCount::Exact(0), HeldCount::Exact(0));
        assert!(inventory.is_clear());
        assert_eq!(inventory.buckets().count(), RefusalKind::ALL.len() + 1);
        assert_eq!(
            inventory.set_aside(SetAsideBucket::Unexplained),
            HeldCount::Exact(0)
        );
    }

    #[test]
    fn one_unexplained_row_makes_the_table_unclear() {
        let inventory = OutboxInventory::new(
            BTreeMap::from([(SetAsideBucket::Unexplained, HeldCount::Exact(1))]),
            HeldCount::Exact(0),
            HeldCount::Exact(0),
        );
        assert!(!inventory.is_clear());
    }

    #[test]
    fn a_row_deferred_beyond_policy_makes_the_table_unclear() {
        let inventory = OutboxInventory::new(
            BTreeMap::new(),
            HeldCount::AtLeast(10_000),
            HeldCount::Exact(0),
        );
        assert!(!inventory.is_clear());
        assert_eq!(inventory.beyond_policy().at_least(), 10_000);
    }

    #[test]
    fn a_row_a_claim_skipped_makes_the_table_unclear() {
        let inventory =
            OutboxInventory::new(BTreeMap::new(), HeldCount::Exact(0), HeldCount::Exact(1));
        assert!(!inventory.is_clear());
        assert_eq!(inventory.skipped(), HeldCount::Exact(1));
    }
}
