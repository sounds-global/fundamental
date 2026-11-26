/// The environment a registration token was issued in.
///
/// An APNs token is valid only in the environment it was issued in; single-environment services
/// leave it unset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PushEnvironment {
    /// The development host — a debug build, or TestFlight run from Xcode.
    Sandbox,
    /// The production host — App Store and TestFlight distribution.
    Production,
}
