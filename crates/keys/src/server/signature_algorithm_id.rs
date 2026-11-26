use std::fmt;

use crate::KeyError;

/// The runtime identity of a [`SignatureAlgorithm`](super::SignatureAlgorithm), for
/// storage, key sets and wire headers.
///
/// Code names the algorithm in the type; this value is for where it crosses a boundary as data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SignatureAlgorithmId {
    /// Ed25519 (RFC 8032).
    Ed25519,
    /// ECDSA over NIST P-256 with SHA-256 (FIPS 186-5, RFC 7518 §3.4).
    EcdsaP256Sha256,
    /// RSASSA-PKCS1-v1_5 with SHA-256 (RFC 8017 §8.2, RFC 7518 §3.3).
    RsaPkcs1Sha256,
}

impl SignatureAlgorithmId {
    /// The stable storage token (`ed25519` / `ecdsa-p256-sha256`).
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ed25519 => "ed25519",
            Self::EcdsaP256Sha256 => "ecdsa-p256-sha256",
            Self::RsaPkcs1Sha256 => "rsa-pkcs1-sha256",
        }
    }

    /// The JOSE `alg` value this algorithm is registered under (RFC 7518, RFC 8037).
    #[must_use]
    pub fn jws_alg(&self) -> &'static str {
        match self {
            Self::Ed25519 => "EdDSA",
            Self::EcdsaP256Sha256 => "ES256",
            Self::RsaPkcs1Sha256 => "RS256",
        }
    }

    /// Parses a storage token.
    ///
    /// # Errors
    /// Returns [`KeyError::Malformed`] for a token that names no known algorithm.
    pub fn parse(token: &str) -> Result<Self, KeyError> {
        match token {
            "ed25519" => Ok(Self::Ed25519),
            "ecdsa-p256-sha256" => Ok(Self::EcdsaP256Sha256),
            "rsa-pkcs1-sha256" => Ok(Self::RsaPkcs1Sha256),
            other => Err(KeyError::Malformed(format!(
                "unknown signature algorithm: {other}"
            ))),
        }
    }
}

impl fmt::Display for SignatureAlgorithmId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_token_round_trips_through_parse() {
        for id in [
            SignatureAlgorithmId::Ed25519,
            SignatureAlgorithmId::EcdsaP256Sha256,
            SignatureAlgorithmId::RsaPkcs1Sha256,
        ] {
            assert_eq!(
                SignatureAlgorithmId::parse(id.as_str()).expect("parses"),
                id
            );
        }
    }

    #[test]
    fn the_jose_names_are_the_registered_ones() {
        assert_eq!(SignatureAlgorithmId::Ed25519.jws_alg(), "EdDSA");
        assert_eq!(SignatureAlgorithmId::EcdsaP256Sha256.jws_alg(), "ES256");
        assert_eq!(SignatureAlgorithmId::RsaPkcs1Sha256.jws_alg(), "RS256");
    }

    #[test]
    fn an_unknown_token_is_refused() {
        assert!(matches!(
            SignatureAlgorithmId::parse("ES256"),
            Err(KeyError::Malformed(_))
        ));
    }
}
