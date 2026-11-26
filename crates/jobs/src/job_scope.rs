use std::sync::Arc;

#[cfg(feature = "test-support")]
use crate::JobConfinement;
use crate::{JobBucket, JobKind};

/// What one claim of the job table reaches: one bucket, or every bucket for an inventory, and
/// only the kinds this binary can fire.
#[derive(Clone, Debug)]
pub struct JobScope {
    bucket: Option<JobBucket>,
    kinds: Arc<[&'static str]>,
    #[cfg(feature = "test-support")]
    confinement: Option<JobConfinement>,
}

impl JobScope {
    /// Every bucket, for the kinds `K` declares.
    #[must_use]
    pub fn of<K: JobKind>() -> Self {
        Self {
            bucket: None,
            kinds: K::OPTIONS.iter().map(|kind| kind.value()).collect(),
            #[cfg(feature = "test-support")]
            confinement: None,
        }
    }

    /// Every bucket, for exactly `kinds`: the union of the sets one surface serves.
    pub(crate) fn of_kinds(kinds: impl IntoIterator<Item = &'static str>) -> Self {
        Self {
            bucket: None,
            kinds: kinds.into_iter().collect(),
            #[cfg(feature = "test-support")]
            confinement: None,
        }
    }

    /// This scope narrowed to `bucket`.
    #[must_use]
    pub fn in_bucket(&self, bucket: JobBucket) -> Self {
        Self {
            bucket: Some(bucket),
            kinds: Arc::clone(&self.kinds),
            #[cfg(feature = "test-support")]
            confinement: self.confinement,
        }
    }

    /// This scope reaching only the jobs about `confinement`'s owner.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn confined_to(&self, confinement: JobConfinement) -> Self {
        Self {
            bucket: self.bucket,
            kinds: Arc::clone(&self.kinds),
            confinement: Some(confinement),
        }
    }

    /// The owner this scope is confined to, if any.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn confinement(&self) -> Option<JobConfinement> {
        self.confinement
    }

    /// The bucket this scope is narrowed to, if any.
    #[must_use]
    pub fn bucket(&self) -> Option<JobBucket> {
        self.bucket
    }

    /// The kinds this scope reaches.
    #[must_use]
    pub fn kinds(&self) -> &[&'static str] {
        &self.kinds
    }
}
