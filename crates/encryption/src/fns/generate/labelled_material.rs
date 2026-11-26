use zeroize::Zeroizing;

/// A fresh 32-byte key for `purpose` at `version`, written `<purpose>:<version>:<64 hex>`: the
/// form a [`Keyring`](crate::Keyring) reads from a key file, refusing it in any other slot.
///
/// # Panics
///
/// Panics if the system random number generator fails to produce key material.
#[must_use]
pub fn mint_labelled_material(purpose: &str, version: u16) -> Zeroizing<String> {
    use std::fmt::Write as _;

    let key = Zeroizing::new(crate::generate_dek());
    let mut labelled = Zeroizing::new(format!("{purpose}:{version}:"));
    for byte in key.iter() {
        let _ = write!(labelled, "{byte:02x}");
    }
    labelled
}

#[cfg(test)]
mod tests {
    use super::mint_labelled_material;
    use crate::fns::read_labelled_material;

    #[test]
    fn a_minted_key_is_read_back_in_its_own_slot_only() {
        let minted = mint_labelled_material("example.live-handle.v1", 2);
        assert!(read_labelled_material(&minted, "example.live-handle.v1", "2").is_ok());
        assert!(read_labelled_material(&minted, "example.other.v1", "2").is_err());
        assert!(read_labelled_material(&minted, "example.live-handle.v1", "1").is_err());
    }

    #[test]
    fn every_mint_is_a_new_key() {
        assert_ne!(
            *mint_labelled_material("example.v1", 1),
            *mint_labelled_material("example.v1", 1)
        );
    }
}
