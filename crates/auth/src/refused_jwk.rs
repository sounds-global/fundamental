//! [`RefusedJwk`] — why a key in a JWKS document is not used to verify anything.

/// Why a key in a JWKS document was left out of the key set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RefusedJwk {
    /// An `oct` key: a published symmetric key is a secret anybody can sign with.
    Symmetric,
    /// `use` is not `sig`, `key_ops` does not include `verify`, or `alg` names an encryption
    /// algorithm.
    NotForSignatures,
    /// A curve no signature algorithm here verifies, or one foreign to the key's type.
    UnsupportedCurve,
    /// `alg` names an algorithm this key's type and curve cannot verify.
    AlgorithmMismatch,
    /// No `kid`, in a document holding more than one key: no token could name it.
    Unnamed,
    /// Another accepted key carries the same `kid`, so a token naming it is ambiguous.
    DuplicateKid,
    /// The key is not a JWK this verifier can read; carries the parser's message.
    Malformed(String),
}

impl RefusedJwk {
    pub(crate) const fn as_str(&self) -> &'static str {
        match self {
            Self::Symmetric => "symmetric",
            Self::NotForSignatures => "not_for_signatures",
            Self::UnsupportedCurve => "unsupported_curve",
            Self::AlgorithmMismatch => "algorithm_mismatch",
            Self::Unnamed => "unnamed",
            Self::DuplicateKid => "duplicate_kid",
            Self::Malformed(_) => "malformed",
        }
    }

    pub(crate) fn detail(&self) -> Option<&str> {
        match self {
            Self::Malformed(detail) => Some(detail),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_refusal_logs_as_its_own_class() {
        let all = [
            RefusedJwk::Symmetric,
            RefusedJwk::NotForSignatures,
            RefusedJwk::UnsupportedCurve,
            RefusedJwk::AlgorithmMismatch,
            RefusedJwk::Unnamed,
            RefusedJwk::DuplicateKid,
            RefusedJwk::Malformed(String::new()),
        ];
        let mut classes: Vec<_> = all.iter().map(RefusedJwk::as_str).collect();
        classes.sort_unstable();
        classes.dedup();
        assert_eq!(classes.len(), all.len());
        assert_eq!(RefusedJwk::Symmetric.as_str(), "symmetric");
    }

    #[test]
    fn only_a_malformed_key_carries_a_detail() {
        assert_eq!(
            RefusedJwk::Malformed("missing field `n`".to_owned()).detail(),
            Some("missing field `n`")
        );
        assert_eq!(RefusedJwk::Symmetric.detail(), None);
    }
}
