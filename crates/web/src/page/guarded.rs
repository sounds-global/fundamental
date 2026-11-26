use super::guard_state::GuardState;
use super::sealed::Sealed;

/// The state of a policy that declares at least one `authorize(...)`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Guarded;

impl Sealed for Guarded {}
impl GuardState for Guarded {}
