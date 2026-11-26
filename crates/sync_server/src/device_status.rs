/// The lifecycle state of an enrolled device key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceStatus {
    /// The key is current and its signatures verify.
    Active,
    /// The key was rotated out; its signatures verify only until the grace deadline.
    Grace,
    /// The key is revoked; its signatures never verify again.
    Revoked,
}
