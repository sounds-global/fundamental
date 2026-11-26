//! Canonical action strings for the sync wire protocol.

/// Authenticate the connection (must be the first frame); carries the token.
pub const AUTH: &str = "auth";

/// Pull a slice's changed rows since a high-water mark.
pub const SLICE_PULL: &str = "slice.pull";

/// Push a locally-changed row through a bidirectional slice.
pub const SLICE_UPSERT: &str = "slice.upsert";

/// Subscribe to live [`SLICE_CHANGE`] pushes for a slice.
pub const SLICE_SUBSCRIBE: &str = "slice.subscribe";

/// An unsolicited server push carrying one live change to a subscribed slice.
pub const SLICE_CHANGE: &str = "slice.change";

/// Enroll a new device public key, binding it to the authenticated user.
pub const DEVICE_ENROLL: &str = "device.enroll";

/// Rotate a device key: enroll a new public key and move the old one to grace.
pub const DEVICE_ROTATE: &str = "device.rotate";

/// Revoke a device key immediately.
pub const DEVICE_REVOKE: &str = "device.revoke";

/// Request an E2EE-vault ownership challenge.
pub const VAULT_CHALLENGE: &str = "vault.challenge";

/// Prove E2EE-vault ownership by signing the challenge nonce.
pub const VAULT_AUTH: &str = "vault.auth";
