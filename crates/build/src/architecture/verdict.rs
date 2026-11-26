//! [`Verdict`], what an architecture check decided when it did not refuse.

/// What an architecture check decided when it did not refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Every page check held and every configured dependency rule passed.
    Passed,
}
