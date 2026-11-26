use crate::KeyError;

/// The SLIP-0010 hardened-index offset; Ed25519 derives only hardened children.
const HARDENED_OFFSET: u32 = 0x8000_0000;

/// An ordered list of hardened child indices — e.g. `m/44'/…'/…'`.
///
/// Children are stored as plain numbers, so `Display` reads `m/44'/…'`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivationPath {
    children: Vec<u32>,
}

impl DerivationPath {
    /// Builds a path of hardened children from their plain indices (in order).
    ///
    /// # Errors
    /// Returns [`KeyError::Malformed`] if any child is `>= 2^31`.
    pub fn hardened(children: &[u32]) -> Result<Self, KeyError> {
        if let Some(bad) = children.iter().find(|&&c| c >= HARDENED_OFFSET) {
            return Err(KeyError::Malformed(format!(
                "derivation index {bad} is >= 2^31; Ed25519 paths are hardened-only"
            )));
        }
        Ok(Self {
            children: children.to_vec(),
        })
    }

    /// The indices with the hardened bit set, as the SLIP-0010 chain walks them.
    pub(crate) fn hardened_indices(&self) -> impl Iterator<Item = u32> + '_ {
        self.children.iter().map(|c| c | HARDENED_OFFSET)
    }
}

impl std::fmt::Display for DerivationPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("m")?;
        for child in &self.children {
            write!(f, "/{child}'")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    #[test]
    fn builds_a_hardened_path_and_sets_the_bit() {
        let path = DerivationPath::hardened(&[44, 5_689_413, 0, 0, 0]).expect("valid");
        let indices: Vec<u32> = path.hardened_indices().collect();
        assert_eq!(
            indices,
            vec![
                44 | HARDENED_OFFSET,
                5_689_413 | HARDENED_OFFSET,
                HARDENED_OFFSET,
                HARDENED_OFFSET,
                HARDENED_OFFSET,
            ]
        );
    }

    #[test]
    fn displays_as_a_hardened_path() {
        let path = DerivationPath::hardened(&[44, 0]).expect("valid");
        assert_eq!(path.to_string(), "m/44'/0'");
        assert_eq!(
            DerivationPath::hardened(&[]).expect("root").to_string(),
            "m"
        );
    }

    #[test]
    fn rejects_an_index_that_would_collide_with_the_hardened_bit() {
        let err = DerivationPath::hardened(&[HARDENED_OFFSET]).expect_err("out of range");
        assert!(matches!(err, KeyError::Malformed(_)));
        assert_eq!(err.error_code(), "KEYS_003");
        assert_eq!(err.http_status_code(), 400);
    }
}
