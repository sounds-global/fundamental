/// What a refused reader can do about the refusal, which is what the gate page tells them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GateReason {
    /// Nobody is signed in, and this page needs someone: sign in.
    SignIn,
    /// Someone is signed in, and this page needs them to confirm it is them first.
    StepUp,
    /// The reader may not see this page; there is nothing to do here.
    Forbidden,
}
