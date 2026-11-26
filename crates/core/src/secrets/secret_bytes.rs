//! Secret bytes alias.

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use super::Secret;

/// A secret binary value.
///
/// As a `#[derive(Model)]` field it is encrypted at rest; a bare `Secret<Vec<u8>>` does not compile there.
///
/// # Example
///
/// ```
/// use fundamental_core::secrets::{Secret, SecretBytes};
///
/// let key_material: SecretBytes = Secret::new(vec![0x00, 0x01, 0x02, 0xff]);
///
/// // Redacted everywhere it could leak.
/// assert_eq!(format!("{key_material:?}"), "[REDACTED]");
///
/// // Read the bytes explicitly.
/// assert_eq!(key_material.expose(), &[0x00, 0x01, 0x02, 0xff]);
/// ```
pub type SecretBytes = Secret<Vec<u8>>;
