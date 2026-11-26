use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

const KEY_LEN: usize = 32;

/// Reads `<purpose>:<version>:<64 hex>` material, refusing it in any other slot; a refusal
/// never quotes the key.
pub(crate) fn read_labelled_material(
    labelled: &str,
    purpose: &str,
    version: &str,
) -> Result<Zeroizing<[u8; KEY_LEN]>, String> {
    let Some((labelled_purpose, labelled_version, hex)) = split_label(labelled.trim()) else {
        return Err(
            "expected `<purpose>:<version>:<64 hex>`, as the mint command writes it".to_owned(),
        );
    };
    if labelled_purpose != purpose {
        return Err(format!(
            "the material is labelled for `{labelled_purpose}`, not `{purpose}`"
        ));
    }
    if labelled_version != version {
        return Err(format!(
            "the material is labelled as version `{labelled_version}`"
        ));
    }
    match decode_key(hex) {
        Some(material) if bool::from(material.ct_eq(&[0_u8; KEY_LEN])) => {
            Err("an all-zero key is a placeholder, not a key".to_owned())
        }
        Some(material) => Ok(material),
        None => Err("expected 64 hex characters".to_owned()),
    }
}

fn split_label(labelled: &str) -> Option<(&str, &str, &str)> {
    let mut parts = labelled.rsplitn(3, ':');
    let hex = parts.next()?;
    let version = parts.next()?;
    let purpose = parts.next()?;
    Some((purpose, version, hex))
}

fn decode_key(hex: &str) -> Option<Zeroizing<[u8; KEY_LEN]>> {
    let digits = hex.as_bytes();
    if digits.len() != KEY_LEN * 2 {
        return None;
    }
    let mut key = Zeroizing::new([0_u8; KEY_LEN]);
    for (byte, pair) in key.iter_mut().zip(digits.chunks_exact(2)) {
        let [high, low] = pair else {
            return None;
        };
        *byte = (nibble(*high)? << 4) | nibble(*low)?;
    }
    Some(key)
}

fn nibble(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::read_labelled_material;

    fn labelled(purpose: &str, version: u32, hex: &str) -> String {
        format!("{purpose}:{version}:{hex}")
    }

    #[test]
    fn material_labelled_for_its_slot_is_read() {
        let material =
            read_labelled_material(&labelled("p.one", 1, &"ab".repeat(32)), "p.one", "1")
                .expect("labelled for this slot");
        assert_eq!(*material, [0xab_u8; 32]);
    }

    #[test]
    fn material_labelled_for_another_purpose_names_both_and_never_the_key() {
        let hex = "cd".repeat(32);
        let problem = read_labelled_material(&labelled("p.two", 1, &hex), "p.one", "1")
            .expect_err("another purpose");
        assert!(
            problem.contains("p.two") && problem.contains("p.one"),
            "{problem}"
        );
        assert!(!problem.contains(&hex), "{problem}");
    }

    #[test]
    fn every_other_malformation_is_refused() {
        let hex = "ef".repeat(32);
        for (material, expected) in [
            (labelled("p.one", 2, &hex), "version `2`"),
            (hex.clone(), "expected `<purpose>"),
            (labelled("p.one", 1, &"00".repeat(32)), "all-zero"),
            (labelled("p.one", 1, "abcd"), "64 hex"),
            (labelled("p.one", 1, &"zz".repeat(32)), "64 hex"),
        ] {
            let problem = read_labelled_material(&material, "p.one", "1")
                .expect_err("malformed material is refused");
            assert!(problem.contains(expected), "{expected}: {problem}");
        }
    }
}
