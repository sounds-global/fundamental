use subtle::ConstantTimeEq;

use crate::fns::material_fingerprint;

/// Whether `material`'s fingerprint is `declared`, 64 hex characters in either case.
///
/// Shared by every key source; a malformed declaration matches nothing; constant-time.
pub(crate) fn matches_declared_fingerprint(declared: &str, material: &[u8]) -> bool {
    let declared = declared.trim();
    if declared.len() != 64 {
        return false;
    }
    let mut expected = [0u8; 32];
    for (byte, pair) in expected.iter_mut().zip(declared.as_bytes().chunks_exact(2)) {
        let Some(value) = std::str::from_utf8(pair)
            .ok()
            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
        else {
            return false;
        };
        *byte = value;
    }
    bool::from(material_fingerprint(material).ct_eq(&expected))
}

#[cfg(test)]
mod tests {
    use super::matches_declared_fingerprint;
    use crate::fns::material_fingerprint;

    fn hex(bytes: [u8; 32]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn the_material_matches_its_own_fingerprint_in_either_case() {
        let material = [0x42_u8; 32];
        let declared = hex(material_fingerprint(&material));
        assert!(matches_declared_fingerprint(&declared, &material));
        assert!(matches_declared_fingerprint(
            &declared.to_uppercase(),
            &material
        ));
        assert!(matches_declared_fingerprint(
            &format!(" {declared}\n"),
            &material
        ));
    }

    #[test]
    fn other_material_or_a_malformed_declaration_matches_nothing() {
        let material = [0x42_u8; 32];
        let other = hex(material_fingerprint(&[0x43_u8; 32]));
        assert!(!matches_declared_fingerprint(&other, &material));
        for malformed in ["", "00", &"zz".repeat(32), &"0".repeat(63), &"0".repeat(66)] {
            assert!(
                !matches_declared_fingerprint(malformed, &material),
                "{malformed:?}"
            );
        }
    }
}
