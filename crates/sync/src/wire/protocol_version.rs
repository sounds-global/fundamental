//! The sync wire protocol version.

/// The version of the sync wire protocol this build speaks; bump it when the signed bytes change.
pub const PROTOCOL_VERSION: u32 = 2;

/// Every protocol version this build can decode; add one only with its decoding path.
pub const SUPPORTED_VERSIONS: &[u32] = &[PROTOCOL_VERSION];
