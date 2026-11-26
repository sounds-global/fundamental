use fundamental_db::ephemeral::EphemeralNameError;
use fundamental_db::ident::is_valid_identifier;

/// The name of one pooled schema, `<prefix>pool_<digest>_<slot>`.
///
/// Disjoint from the ephemeral grammar, so no age sweep can take a schema the pool still leases,
/// and deterministic, so the registry's primary key arbitrates slots.
///
/// ```text
/// layer5_test_pool_20f605805a75_3
/// │           │    │            └── the slot: which of the pool's schemas this is
/// │           │    └── 12 hex characters of the migration set's digest
/// │           └── the marker, which is what makes the two grammars disjoint
/// └── the caller's prefix, including its trailing separator
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PooledSchemaName(String);

impl PooledSchemaName {
    /// The segment that separates this grammar from the ephemeral one.
    pub const MARKER: &'static str = "pool_";

    /// Hex characters of the migration digest kept in the name; a label, as the registry holds the full digest.
    pub const DIGEST_CHARS: usize = 12;

    /// Mints the name of the `slot`th schema of the pool for `fingerprint`; `prefix` includes its trailing separator.
    ///
    /// # Errors
    /// [`EphemeralNameError::InvalidPrefix`] if `prefix` is not a plain identifier or
    /// `fingerprint` is not lower-case hex; [`PrefixNotSeparated`](EphemeralNameError::PrefixNotSeparated)
    /// if `prefix` does not end with `_`; and [`TooLong`](EphemeralNameError::TooLong) if the
    /// minted name would exceed `max_len`.
    pub fn mint(
        prefix: &str,
        fingerprint: &str,
        slot: u32,
        max_len: usize,
    ) -> Result<Self, EphemeralNameError> {
        if !is_valid_identifier(prefix) {
            return Err(EphemeralNameError::InvalidPrefix {
                prefix: prefix.to_owned(),
            });
        }
        if !prefix.ends_with('_') {
            return Err(EphemeralNameError::PrefixNotSeparated {
                prefix: prefix.to_owned(),
            });
        }
        let digest: String = fingerprint.chars().take(Self::DIGEST_CHARS).collect();
        if digest.len() != Self::DIGEST_CHARS || !is_lower_hex(&digest) {
            return Err(EphemeralNameError::InvalidPrefix {
                prefix: fingerprint.to_owned(),
            });
        }

        let marker = Self::MARKER;
        let name = format!("{prefix}{marker}{digest}_{slot}");
        if name.len() > max_len {
            return Err(EphemeralNameError::TooLong {
                prefix: prefix.to_owned(),
                actual: name.len(),
                max_len,
            });
        }
        Ok(Self(name))
    }

    /// The name, as it goes into DDL.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether `name` was minted here, whatever its prefix; anything else reads as `false`.
    #[must_use]
    pub fn is_pooled(name: &str) -> bool {
        let Some((head, slot)) = name.rsplit_once('_') else {
            return false;
        };
        if slot.is_empty() || !slot.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        let Some((head, digest)) = head.rsplit_once('_') else {
            return false;
        };
        if digest.len() != Self::DIGEST_CHARS || !is_lower_hex(digest) {
            return false;
        }
        // `_pool`: a prefix that merely ends in those letters is not one of these.
        head.ends_with("_pool")
    }
}

impl core::fmt::Display for PooledSchemaName {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Lower-case only, as `content_hash` emits, so one digest never names two schemas.
fn is_lower_hex(value: &str) -> bool {
    value
        .chars()
        .all(|c| c.is_ascii_digit() || (c.is_ascii_lowercase() && c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use fundamental_db::ephemeral::EphemeralName;

    use super::*;

    const DIGEST: &str = "20f605805a75b365afab8c3f9e7a69292589833c3561a78d7d79af83a0ad5d7c";

    #[test]
    fn a_minted_name_reads_back_as_pooled() {
        let name = PooledSchemaName::mint("layer5_test_", DIGEST, 3, 63).expect("a valid name");
        assert_eq!(name.as_str(), "layer5_test_pool_20f605805a75_3");
        assert!(PooledSchemaName::is_pooled(name.as_str()));
    }

    #[test]
    fn no_pooled_name_is_ever_readable_as_an_ephemeral_one() {
        for slot in [0, 9, 15, 99, 999] {
            for prefix in [
                "t_",
                "layer5_test_",
                "bu_pool_probe_",
                "layer5_mirror_test_",
            ] {
                let name = PooledSchemaName::mint(prefix, DIGEST, slot, 63).expect("a valid name");
                assert!(
                    EphemeralName::stamp_of(name.as_str()).is_none(),
                    "{name} would be swept by age while the pool still leases it"
                );
            }
        }
    }

    /// A twelve-character digest is all digits about once in 270, which is what the ephemeral grammar anchors on.
    #[test]
    fn an_all_digit_digest_is_still_not_an_ephemeral_name() {
        let name = PooledSchemaName::mint("t_", "178782375943", 0, 63).expect("a valid name");
        assert_eq!(name.as_str(), "t_pool_178782375943_0");
        assert!(EphemeralName::stamp_of(name.as_str()).is_none());
    }

    #[test]
    fn an_ephemeral_name_is_never_read_as_pooled() {
        let minted = EphemeralName::mint("layer5_test_", 63).expect("a valid prefix");
        assert!(!PooledSchemaName::is_pooled(minted.as_str()));
    }

    #[test]
    fn a_held_name_is_never_read_as_pooled() {
        use fundamental_db::ephemeral::HeldName;

        for prefix in [
            "s_",
            "t_",
            "layer5_test_",
            "bu_pool_probe_",
            "layer5_mirror_test_",
        ] {
            for _ in 0..64 {
                let held = HeldName::mint(prefix, 63).expect("a held name");
                assert!(!PooledSchemaName::is_pooled(held.as_str()), "{held}");
            }
        }
    }

    #[test]
    fn nothing_outside_the_grammar_is_pooled() {
        for name in [
            "public",
            "information_schema",
            "layer5",
            "layer5_test_pool_20f605805a75",
            "layer5_test_pool_20F605805A75_3",
            "layer5_test_pool_20f605805a7_3",
            "pool_20f605805a75_3",
            "layer5_test_pool_20f605805a75_x",
        ] {
            assert!(!PooledSchemaName::is_pooled(name), "{name}");
        }
    }

    #[test]
    fn a_prefix_that_could_end_a_literal_is_refused_before_anything_is_created() {
        let refused = PooledSchemaName::mint("t'; DROP SCHEMA prod; --", DIGEST, 0, 63);
        assert!(matches!(
            refused,
            Err(EphemeralNameError::InvalidPrefix { .. })
        ));
    }

    #[test]
    fn a_prefix_without_its_separator_is_refused() {
        let refused = PooledSchemaName::mint("layer5", DIGEST, 0, 63);
        assert!(matches!(
            refused,
            Err(EphemeralNameError::PrefixNotSeparated { .. })
        ));
    }

    #[test]
    fn a_digest_that_is_not_lower_case_hex_is_refused() {
        for digest in ["not-a-digest", "20F605805A75", "20f6"] {
            let refused = PooledSchemaName::mint("t_", digest, 0, 63);
            assert!(
                matches!(refused, Err(EphemeralNameError::InvalidPrefix { .. })),
                "{digest}"
            );
        }
    }

    #[test]
    fn a_name_the_engine_would_refuse_is_reported_as_too_long() {
        let refused = PooledSchemaName::mint("t_", DIGEST, 0, 8);
        assert!(matches!(refused, Err(EphemeralNameError::TooLong { .. })));
    }
}
