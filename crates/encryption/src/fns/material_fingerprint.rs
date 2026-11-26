/// A non-secret BLAKE3-derived fingerprint of key material, the same wherever it is configured.
#[must_use]
pub fn material_fingerprint(material: &[u8]) -> [u8; 32] {
    blake3::derive_key("fundamental key material 2026-09-13 fingerprint", material)
}

#[cfg(test)]
mod tests {
    use super::material_fingerprint;

    #[test]
    fn a_fingerprint_is_deterministic_and_is_not_the_material() {
        let material = [0x42_u8; 32];
        assert_eq!(
            material_fingerprint(&material),
            material_fingerprint(&material)
        );
        assert_ne!(material_fingerprint(&material), material);
        assert_ne!(
            material_fingerprint(&material),
            material_fingerprint(&[0x43_u8; 32])
        );
    }
}
