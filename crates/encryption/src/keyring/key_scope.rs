use crate::KeyringError;
use std::fmt;

/// The deployment a key belongs to when one binary runs as several, such as production and a
/// sandbox.
///
/// A scoped purpose (`acme.live-handle.v1@sandbox`) changes the material's label and every
/// derived key, so deployments share nothing; no scope is the unscoped names.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct KeyScope(Box<str>);

impl KeyScope {
    /// The longest scope name accepted.
    pub const MAX_LEN: usize = 32;

    /// The scope named `name`: a lower-case ASCII letter, then letters, digits and single hyphens,
    /// ending in a letter or digit, at most [`MAX_LEN`](Self::MAX_LEN) long.
    ///
    /// The alphabet excludes `@` and `:`, the label's separators.
    ///
    /// # Errors
    /// [`KeyringError::InvalidScope`] for any other name.
    pub fn new(name: &str) -> Result<Self, KeyringError> {
        let bytes = name.as_bytes();
        let well_formed = bytes.len() <= Self::MAX_LEN
            && bytes.first().is_some_and(u8::is_ascii_lowercase)
            && bytes
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
            && !name.ends_with('-')
            && !name.contains("--");
        if well_formed {
            Ok(Self(name.into()))
        } else {
            Err(KeyringError::InvalidScope {
                scope: name.chars().take(Self::MAX_LEN + 1).collect(),
            })
        }
    }

    /// The scope's name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `purpose` as this scope names it: `<purpose>@<scope>`.
    #[must_use]
    pub fn qualify(&self, purpose: &str) -> String {
        format!("{purpose}@{}", self.0)
    }
}

impl fmt::Debug for KeyScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("KeyScope").field(&&*self.0).finish()
    }
}

impl fmt::Display for KeyScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::KeyScope;
    use crate::KeyringError;
    use fundamental_core::Error;

    #[test]
    fn a_scope_qualifies_a_purpose_after_an_at_sign() {
        let scope = KeyScope::new("sandbox").expect("a well-formed scope");
        assert_eq!(scope.as_str(), "sandbox");
        assert_eq!(
            scope.qualify("acme.live-handle.v1"),
            "acme.live-handle.v1@sandbox"
        );
    }

    #[test]
    fn letters_digits_and_single_inner_hyphens_are_accepted() {
        let longest = "a".repeat(KeyScope::MAX_LEN);
        for name in ["a", "sandbox", "eu-west-2", "cell9", longest.as_str()] {
            assert!(KeyScope::new(name).is_ok(), "`{name}` is a scope");
        }
    }

    #[test]
    fn every_other_name_is_refused_with_its_code() {
        let too_long = "a".repeat(KeyScope::MAX_LEN + 1);
        for name in [
            "",
            "Sandbox",
            "9cell",
            "-sandbox",
            "sandbox-",
            "sand--box",
            "sand box",
            "sand@box",
            "sand:box",
            "sandbox\n",
            "zandbóx",
            too_long.as_str(),
        ] {
            let refused = KeyScope::new(name).expect_err(name);
            assert!(
                matches!(refused, KeyringError::InvalidScope { .. }),
                "{name:?}"
            );
            assert_eq!(refused.error_code(), "KEYRING_006");
        }
    }

    #[test]
    fn a_refused_name_is_echoed_bounded() {
        let long = "a".repeat(10_000);
        let KeyringError::InvalidScope { scope } = KeyScope::new(&long).expect_err("too long")
        else {
            panic!("a scope refusal is InvalidScope");
        };
        assert_eq!(scope.chars().count(), KeyScope::MAX_LEN + 1);
    }
}
