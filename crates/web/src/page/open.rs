use super::guard_state::GuardState;
use super::sealed::Sealed;

/// The state of a policy that declares no `authorize(...)`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Open;

impl Sealed for Open {}
impl GuardState for Open {}
