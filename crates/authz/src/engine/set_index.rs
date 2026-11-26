//! [`SetIndex`]: the seam a precomputed set-membership index plugs into.

use std::fmt::Debug;

use crate::schema::{NamespaceName, RelationName};
use crate::tuple::{ObjectRef, SubjectRef};
use crate::zookie::HybridTs;

/// A precomputed transitive-membership index — Zanzibar's Leopard, as a seam.
///
/// Answers a nested membership in one lookup. A safe accelerator, never an authority: it must
/// be exact and complete for every relation it covers as of its watermark, cover only pure
/// transitive membership, and return `None` for anything else.
pub trait SetIndex: Send + Sync + Debug {
    /// The timestamp the index's contents reflect.
    fn watermark(&self) -> HybridTs;

    /// Whether the index holds an exact, complete closure for `namespace#relation`.
    fn covers(&self, namespace: &NamespaceName, relation: &RelationName) -> bool;

    /// Whether `subject` is in the transitive closure of `object#relation`.
    ///
    /// `None` when the index cannot answer, and the caller evaluates authoritatively.
    fn contains(
        &self,
        object: &ObjectRef,
        relation: &RelationName,
        subject: &SubjectRef,
    ) -> Option<bool>;
}
