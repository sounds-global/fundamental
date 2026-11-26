use crate::{CaseFold, Fold};

/// What one naming space considers the same name; frozen once anything is stored under it.
pub trait SkeletonProfile: 'static {
    /// A stable name for this profile, recorded in its fingerprint.
    const NAME: &'static str;

    /// The longest input this profile folds, in bytes, refused before any work.
    const MAX_BYTES: usize;

    /// Characters deleted before anything else, such as a slug's `-`.
    const REMOVED: &'static [char];

    /// Whether case is meaningful in this naming space.
    const CASE: CaseFold;

    /// Sequence rewrites applied after the UTS #39 skeleton, longest source first.
    const FOLDS: &'static [Fold];
}
