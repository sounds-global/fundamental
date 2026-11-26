//! [`Zookie`]: an opaque consistency token minted on write, honored on read.

use core::fmt;

use serde::{Deserialize, Serialize};

use super::{BackendTag, HybridTs};
use crate::error::AuthzError;
use crate::schema::SchemaHash;

/// The wire-format version prefix.
const VERSION: &str = "z1";

/// An opaque consistency token — a "zookie" in Zanzibar's terminology.
///
/// A later check can pass it back as a freshness floor. It encodes the minting backend, the
/// write's [`HybridTs`] and the [`SchemaHash`] in force; callers treat it as opaque.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Zookie {
    backend: BackendTag,
    ts: HybridTs,
    schema_hash: SchemaHash,
}

impl Zookie {
    /// Mints a zookie for a write on `backend` at `ts` under `schema_hash`.
    #[must_use]
    pub fn new(backend: BackendTag, ts: HybridTs, schema_hash: SchemaHash) -> Self {
        Self {
            backend,
            ts,
            schema_hash,
        }
    }

    /// The backend that minted this token.
    #[must_use]
    pub fn backend(&self) -> BackendTag {
        self.backend
    }

    /// The write timestamp this token pins to.
    #[must_use]
    pub fn ts(&self) -> HybridTs {
        self.ts
    }

    /// The schema fingerprint in force when this token was minted.
    #[must_use]
    pub fn schema_hash(&self) -> SchemaHash {
        self.schema_hash
    }

    /// Encodes the token to its opaque wire string.
    #[must_use]
    pub fn encode(&self) -> String {
        format!(
            "{VERSION}:{}:{}:{}:{:016x}",
            self.backend.code(),
            self.ts.wall,
            self.ts.logical,
            self.schema_hash.to_u64(),
        )
    }

    /// Decodes an opaque wire string back into a token.
    ///
    /// # Errors
    ///
    /// Returns [`AuthzError::InvalidZookie`] if `raw` is not a well-formed token.
    pub fn decode(raw: &str) -> Result<Self, AuthzError> {
        let invalid = || AuthzError::InvalidZookie {
            value: raw.chars().take(64).collect(),
        };
        let mut parts = raw.split(':');
        if parts.next() != Some(VERSION) {
            return Err(invalid());
        }
        let backend = parts
            .next()
            .and_then(BackendTag::from_code)
            .ok_or_else(invalid)?;
        let wall = parts
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(invalid)?;
        let logical = parts
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(invalid)?;
        let hash = parts
            .next()
            .and_then(|s| u64::from_str_radix(s, 16).ok())
            .ok_or_else(invalid)?;
        if parts.next().is_some() {
            return Err(invalid());
        }
        Ok(Self {
            backend,
            ts: HybridTs::new(wall, logical),
            schema_hash: SchemaHash::from(hash),
        })
    }
}

impl fmt::Display for Zookie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.encode())
    }
}

impl From<Zookie> for String {
    fn from(zookie: Zookie) -> Self {
        zookie.encode()
    }
}

impl TryFrom<String> for Zookie {
    type Error = AuthzError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Zookie::decode(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Zookie {
        Zookie::new(
            BackendTag::SqlDb,
            HybridTs::new(1_700_000_000_000_000_000, 7),
            SchemaHash::from(0xdead_beef),
        )
    }

    #[test]
    fn round_trips_through_the_wire_form() {
        let zookie = sample();
        let encoded = zookie.encode();
        assert_eq!(Zookie::decode(&encoded).expect("decode"), zookie);
    }

    #[test]
    fn round_trips_through_json() {
        let zookie = sample();
        let json = serde_json::to_string(&zookie).expect("serialize");
        assert_eq!(
            serde_json::from_str::<Zookie>(&json).expect("deserialize"),
            zookie
        );
    }

    #[test]
    fn rejects_malformed_tokens() {
        for bad in [
            "",
            "z1",
            "z2:c:1:0:0",
            "z1:x:1:0:0",
            "z1:c:notnum:0:0",
            "z1:c:1:0:0:extra",
        ] {
            assert!(matches!(
                Zookie::decode(bad),
                Err(AuthzError::InvalidZookie { .. })
            ));
        }
    }
}
