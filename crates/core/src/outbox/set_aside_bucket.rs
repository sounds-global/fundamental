//! [`SetAsideBucket`] — the reason an inventory counts a set-aside row under.

use core::fmt;

use super::RefusalKind;

/// The reason an inventory counts a set-aside row under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SetAsideBucket {
    /// Set aside by a claim, for this reason, and tagged by the framework.
    Reason(RefusalKind),
    /// Set aside by something the framework cannot vouch for.
    Unexplained,
}

impl SetAsideBucket {
    /// Every bucket, so a gauge can publish each one at zero.
    pub fn all() -> impl Iterator<Item = Self> {
        RefusalKind::ALL
            .into_iter()
            .map(Self::Reason)
            .chain(core::iter::once(Self::Unexplained))
    }

    /// The label a metric carries for this bucket.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Reason(kind) => kind.code(),
            Self::Unexplained => "unexplained",
        }
    }
}

impl fmt::Display for SetAsideBucket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

#[cfg(test)]
mod tests {
    use super::{RefusalKind, SetAsideBucket};

    #[test]
    fn every_reason_and_the_unexplained_bucket_are_listed_once() {
        let codes: Vec<_> = SetAsideBucket::all().map(SetAsideBucket::code).collect();
        assert_eq!(codes.len(), RefusalKind::ALL.len() + 1);
        assert!(codes.contains(&"unexplained"));
        let mut unique = codes.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), codes.len());
    }

    #[test]
    fn no_refusal_code_collides_with_the_unexplained_label() {
        assert!(
            RefusalKind::ALL
                .iter()
                .all(|kind| kind.code() != "unexplained")
        );
    }
}
