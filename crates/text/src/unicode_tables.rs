/// The Unicode data a fold ran against, recorded wherever a skeleton is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnicodeTables {
    /// The Unicode Standard version the tables implement.
    pub unicode: &'static str,
    /// Where the tables come from, precise enough to reproduce the fold.
    pub source: &'static str,
}

impl UnicodeTables {
    /// The data behind [`Skeleton`](crate::Skeleton): `unicode-security`'s UTS #39 prototypes and `unicode-normalization`, both pinned.
    pub const UTS39: Self = Self {
        unicode: "16.0.0",
        source: "unicode-security 0.1.2 (UTS #39 confusables), unicode-normalization 0.1",
    };
}

#[cfg(test)]
mod tests {
    use super::UnicodeTables;

    #[test]
    fn the_named_version_is_the_one_the_tables_carry() {
        let (major, minor, patch) = unicode_security::UNICODE_VERSION;
        assert_eq!(
            UnicodeTables::UTS39.unicode,
            format!("{major}.{minor}.{patch}"),
            "`unicode-security` carries a Unicode version this constant does not name; \
             every stored skeleton was folded under the old one"
        );
    }
}
