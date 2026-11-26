use core::fmt;

use crate::{SkeletonProfile, UnicodeTables};

/// Dated, so a fingerprint over different material is a different value.
const CONTEXT: &str = "fundamental 2026-09-23 skeleton profile fingerprint";

/// The one name of a profile's behaviour: every table and rule that produced a skeleton.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProfileFingerprint([u8; 16]);

impl ProfileFingerprint {
    /// The fingerprint of `P` under the tables `Skeleton` folds with.
    #[must_use]
    pub fn of<P: SkeletonProfile>() -> Self {
        let mut material = Vec::new();
        part(&mut material, P::NAME.as_bytes());
        part(&mut material, UnicodeTables::UTS39.unicode.as_bytes());
        part(&mut material, UnicodeTables::UTS39.source.as_bytes());
        part(&mut material, &P::MAX_BYTES.to_be_bytes());
        part(&mut material, &[P::CASE.code()]);

        let mut removed = Vec::new();
        for character in P::REMOVED {
            removed.extend_from_slice(&u32::from(*character).to_be_bytes());
        }
        part(&mut material, &removed);

        let mut folds = Vec::new();
        for fold in P::FOLDS {
            part(&mut folds, fold.from.as_bytes());
            part(&mut folds, fold.to.as_bytes());
        }
        part(&mut material, &folds);

        let full = blake3::derive_key(CONTEXT, &material);
        let mut short = [0_u8; 16];
        short.copy_from_slice(&full[..16]);
        Self(short)
    }

    /// The fingerprint's bytes, for a column that stores it.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// The fingerprint as lowercase hex, for a log line or a human-read record.
    #[must_use]
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

fn part(material: &mut Vec<u8>, bytes: &[u8]) {
    material.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    material.extend_from_slice(bytes);
}

impl fmt::Debug for ProfileFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ProfileFingerprint")
            .field(&self.to_hex())
            .finish()
    }
}

impl fmt::Display for ProfileFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::ProfileFingerprint;
    use crate::{CaseFold, Fold, SkeletonProfile};

    struct Base;
    impl SkeletonProfile for Base {
        const NAME: &'static str = "base";
        const MAX_BYTES: usize = 64;
        const REMOVED: &'static [char] = &['-'];
        const CASE: CaseFold = CaseFold::Lowercase;
        const FOLDS: &'static [Fold] = &[Fold::new("rn", "m")];
    }

    struct RenamedOnly;
    impl SkeletonProfile for RenamedOnly {
        const NAME: &'static str = "renamed";
        const MAX_BYTES: usize = 64;
        const REMOVED: &'static [char] = &['-'];
        const CASE: CaseFold = CaseFold::Lowercase;
        const FOLDS: &'static [Fold] = &[Fold::new("rn", "m")];
    }

    struct OneMoreFold;
    impl SkeletonProfile for OneMoreFold {
        const NAME: &'static str = "base";
        const MAX_BYTES: usize = 64;
        const REMOVED: &'static [char] = &['-'];
        const CASE: CaseFold = CaseFold::Lowercase;
        const FOLDS: &'static [Fold] = &[Fold::new("rn", "m"), Fold::new("vv", "w")];
    }

    struct CaseChanged;
    impl SkeletonProfile for CaseChanged {
        const NAME: &'static str = "base";
        const MAX_BYTES: usize = 64;
        const REMOVED: &'static [char] = &['-'];
        const CASE: CaseFold = CaseFold::None;
        const FOLDS: &'static [Fold] = &[Fold::new("rn", "m")];
    }

    #[test]
    fn one_profile_has_one_fingerprint() {
        assert_eq!(
            ProfileFingerprint::of::<Base>(),
            ProfileFingerprint::of::<Base>()
        );
    }

    #[test]
    fn a_profiles_name_is_part_of_its_fingerprint() {
        assert_ne!(
            ProfileFingerprint::of::<Base>(),
            ProfileFingerprint::of::<RenamedOnly>()
        );
    }

    #[test]
    fn adding_a_fold_changes_the_fingerprint() {
        assert_ne!(
            ProfileFingerprint::of::<Base>(),
            ProfileFingerprint::of::<OneMoreFold>()
        );
    }

    #[test]
    fn changing_case_folding_changes_the_fingerprint() {
        assert_ne!(
            ProfileFingerprint::of::<Base>(),
            ProfileFingerprint::of::<CaseChanged>()
        );
    }

    #[test]
    fn hex_is_sixteen_bytes_wide() {
        assert_eq!(ProfileFingerprint::of::<Base>().to_hex().len(), 32);
    }

    #[test]
    fn debug_shows_the_hex() {
        let fingerprint = ProfileFingerprint::of::<Base>();
        let shown = format!("{fingerprint:?}");
        assert!(shown.contains(&fingerprint.to_hex()), "{shown}");
    }
}
