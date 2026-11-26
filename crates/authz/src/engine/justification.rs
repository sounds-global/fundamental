//! [`Justification`]: what a check's walk keeps of an allow.

use crate::tuple::RelationTuple;

/// What the walk carries upward for a sub-question it allowed.
///
/// A plain check keeps `()`; a locked check keeps the [`Witness`](super::Witness) it locks.
pub(crate) trait Justification: Clone + Send + Sync + 'static {
    /// Whether the walk may evaluate an exclusion.
    ///
    /// An exclusion allows on a tuple's absence, which no witness can hold.
    const ADMITS_EXCLUSION: bool;

    /// What an allow that rests on no tuple keeps: the identity of [`join`](Self::join).
    fn unit() -> Self;

    /// The justification of one granting stored edge; `tuple` is built only when kept.
    fn edge(tuple: impl FnOnce() -> RelationTuple) -> Self;

    /// Two justifications that must hold together.
    #[must_use]
    fn join(self, other: Self) -> Self;
}

impl Justification for () {
    const ADMITS_EXCLUSION: bool = true;

    fn unit() -> Self {}

    fn edge(_: impl FnOnce() -> RelationTuple) -> Self {}

    fn join(self, (): Self) -> Self {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_check_never_builds_the_edge() {
        <() as Justification>::edge(|| unreachable!("a plain check keeps no tuple"));
    }
}
