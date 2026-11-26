/// Which roots a [`RemoteBridge`](crate::RemoteBridge) presenting a
/// [`ClientIdentity`](crate::ClientIdentity) authenticates its server against.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServerTrust {
    /// The operating system's trust store.
    Platform,
    /// Only these PEM certificates, pinned.
    Anchors(String),
}
