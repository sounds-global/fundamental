use super::guard_state::GuardState;
use super::guarded::Guarded;
use super::open::Open;

/// The state of a chain once a policy in state `O` joins it: guarded as soon as either is.
pub trait Join<O: GuardState>: GuardState {
    /// The composed state.
    type Output: GuardState;
}

impl Join<Open> for Open {
    type Output = Open;
}

impl Join<Guarded> for Open {
    type Output = Guarded;
}

impl Join<Open> for Guarded {
    type Output = Guarded;
}

impl Join<Guarded> for Guarded {
    type Output = Guarded;
}
