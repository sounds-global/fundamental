//! [`TupleChanged`]: the event announcing that relationship tuples were written.

use serde::{Deserialize, Serialize};

use crate::tuple::ObjectRef;
use crate::zookie::{BackendTag, HybridTs};

/// Announces that one or more relationship tuples changed, at a known commit
/// timestamp.
///
/// `commit_ts` is the new watermark and `objects` what moved, for targeted cache eviction;
/// it carries no relationship data. Published on `authz.events.tuple.changed`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TupleChanged {
    /// The commit timestamp of the write — the watermark caches advance to.
    pub commit_ts: HybridTs,
    /// The backend the write landed on (its timestamps share one clock domain).
    pub backend: BackendTag,
    /// The objects whose relationships changed, for targeted cache eviction.
    #[serde(default)]
    pub objects: Vec<ObjectRef>,
}

impl TupleChanged {
    /// Builds a change event at `commit_ts` on `backend` touching `objects`.
    #[must_use]
    pub fn new(commit_ts: HybridTs, backend: BackendTag, objects: Vec<ObjectRef>) -> Self {
        Self {
            commit_ts,
            backend,
            objects,
        }
    }
}
