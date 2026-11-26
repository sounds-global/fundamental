//! [`NkeyPublicKey`]: a NATS user's public NKey.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Base32 of a prefix byte, 32 key bytes and a two-byte CRC, unpadded.
const ENCODED_LEN: usize = 56;

/// A NATS user public NKey: `U` followed by 55 base32 characters; a seed or account key is refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NkeyPublicKey(String);

impl NkeyPublicKey {
    /// The key as the server's configuration writes it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NkeyPublicKey {
    type Error = String;

    fn try_from(encoded: String) -> Result<Self, Self::Error> {
        if encoded.starts_with('S') {
            return Err(
                "an NKey public_key is the user's public key (U…); this is a seed, which must \
                 never be written in configuration"
                    .to_owned(),
            );
        }
        if !encoded.starts_with('U') {
            return Err("an NKey public_key is a user public key and starts with U".to_owned());
        }
        if encoded.len() != ENCODED_LEN
            || !encoded
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || (b'2'..=b'7').contains(&byte))
        {
            return Err(format!(
                "an NKey public_key is {ENCODED_LEN} base32 characters (A–Z, 2–7)"
            ));
        }
        Ok(Self(encoded))
    }
}

impl From<NkeyPublicKey> for String {
    fn from(key: NkeyPublicKey) -> Self {
        key.0
    }
}

impl fmt::Display for NkeyPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::NkeyPublicKey;

    const DOCUMENTED: &str = "UAPZQH4MNJCOVEJFERB3NFSIROQ5RE7CGBEPKAZSB6QB7IQHBKXHZPVP";

    #[test]
    fn a_user_public_key_is_accepted() {
        let key = NkeyPublicKey::try_from(DOCUMENTED.to_owned()).expect("a user key");
        assert_eq!(key.as_str(), DOCUMENTED);
    }

    #[test]
    fn a_seed_an_account_key_or_a_short_key_is_refused() {
        for (encoded, why) in [
            (
                "SUAMLK2ZNL35WSMW37E7UD4VZ7ELPKW7DHC3BWBSD2GCZ7IUQQXZIORRBU",
                "this is a seed",
            ),
            (&DOCUMENTED.replacen('U', "A", 1), "starts with U"),
            (&DOCUMENTED[..40], "56 base32 characters"),
            (&DOCUMENTED.replacen('A', "a", 1), "56 base32 characters"),
        ] {
            let refused = NkeyPublicKey::try_from(encoded.to_owned()).expect_err(encoded);
            assert!(refused.contains(why), "{encoded}: {refused}");
        }
    }
}
