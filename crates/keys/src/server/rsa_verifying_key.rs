use std::fmt;

use ring::signature::{RSA_PKCS1_2048_8192_SHA256, RsaPublicKeyComponents};
use spki::ObjectIdentifier;
use spki::SubjectPublicKeyInfoRef;
use spki::der::asn1::UintRef;
use spki::der::{Decode as _, Reader as _, SliceReader};
use tracing::warn;

use crate::KeyError;
use crate::server::RsaSignature;

/// `rsaEncryption` (RFC 8017 Appendix C).
const RSA_ENCRYPTION: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");
/// The smallest modulus accepted, in bits: RFC 7518 §3.3's floor for `RS256`.
const MIN_BITS: usize = 2048;
/// The largest modulus accepted, in bits, bounding what one verification costs.
const MAX_BITS: usize = 4096;
/// The only public exponent accepted.
const EXPONENT: [u8; 3] = [0x01, 0x00, 0x01];

/// An RSA public key with a minimally encoded 2048 to 4096-bit modulus and exponent 65537.
#[derive(Clone, PartialEq, Eq)]
pub struct RsaVerifyingKey {
    modulus: Vec<u8>,
}

impl RsaVerifyingKey {
    /// The key with big-endian modulus `n` and exponent `e` — the JWK form (RFC 7518 §6.3.1).
    ///
    /// # Errors
    /// Returns [`KeyError::Malformed`] for a modulus outside 2048–4096 bits or with a leading
    /// zero byte, or an exponent other than 65537.
    pub fn from_components(n: &[u8], e: &[u8]) -> Result<Self, KeyError> {
        if e != EXPONENT {
            return Err(KeyError::Malformed(
                "an RSA verifying key has the exponent 65537".to_owned(),
            ));
        }
        if n.first() == Some(&0) {
            return Err(KeyError::Malformed(
                "an RSA modulus is minimally encoded".to_owned(),
            ));
        }
        let bits = n.len() * 8 - n.first().map_or(8, |top| top.leading_zeros() as usize);
        if !(MIN_BITS..=MAX_BITS).contains(&bits) {
            return Err(KeyError::Malformed(format!(
                "an RSA modulus is {MIN_BITS} to {MAX_BITS} bits, not {bits}"
            )));
        }
        Ok(Self {
            modulus: n.to_vec(),
        })
    }

    /// Parses a PEM `SubjectPublicKeyInfo` document holding an RSA key.
    ///
    /// # Errors
    /// Returns [`KeyError::Malformed`] if the document is not an RSA public key, or the key is
    /// one [`from_components`](Self::from_components) refuses.
    pub fn from_public_key_pem(pem: &str) -> Result<Self, KeyError> {
        let malformed = |what: &str| KeyError::Malformed(format!("not an RSA public key: {what}"));
        let (label, der) =
            spki::der::pem::decode_vec(pem.as_bytes()).map_err(|_| malformed("PEM"))?;
        if label != "PUBLIC KEY" {
            return Err(malformed("PEM label"));
        }
        let info = SubjectPublicKeyInfoRef::from_der(&der)
            .map_err(|_| malformed("SubjectPublicKeyInfo"))?;
        if info.algorithm.oid != RSA_ENCRYPTION {
            return Err(malformed("algorithm"));
        }
        let pkcs1 = info
            .subject_public_key
            .as_bytes()
            .ok_or_else(|| malformed("bit string"))?;
        let (n, e) = pkcs1_components(pkcs1).map_err(|_| malformed("RSAPublicKey"))?;
        Self::from_components(n.as_bytes(), e.as_bytes())
    }

    /// The big-endian modulus, as a JWK's `n` carries it.
    #[must_use]
    pub fn modulus(&self) -> &[u8] {
        &self.modulus
    }

    /// Verifies `signature` over `message` with SHA-256.
    ///
    /// # Errors
    /// Returns [`KeyError::BadSignature`] if it does not verify, a signature of another
    /// length than the modulus included.
    pub fn verify(&self, message: &[u8], signature: &RsaSignature) -> Result<(), KeyError> {
        RsaPublicKeyComponents {
            n: self.modulus.as_slice(),
            e: EXPONENT.as_slice(),
        }
        .verify(&RSA_PKCS1_2048_8192_SHA256, message, signature.as_bytes())
        .map_err(|_| {
            warn!(
                modulus_bytes = self.modulus.len(),
                "an RS256 signature did not verify"
            );
            KeyError::BadSignature
        })
    }
}

/// The modulus and exponent of a PKCS#1 `RSAPublicKey`, and nothing after it.
fn pkcs1_components(der: &[u8]) -> spki::der::Result<(UintRef<'_>, UintRef<'_>)> {
    let mut reader = SliceReader::new(der)?;
    let components = reader.sequence(|inner| {
        let n = UintRef::decode(inner)?;
        let e = UintRef::decode(inner)?;
        Ok((n, e))
    })?;
    reader.finish(components)
}

impl fmt::Debug for RsaVerifyingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RsaVerifyingKey({} bits)", self.modulus.len() * 8)
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    use super::*;

    /// RFC 7515 Appendix A.2: the JWS Signing Input, the key's `n`, and the signature.
    const A2_INPUT: &str = "eyJhbGciOiJSUzI1NiJ9.eyJpc3MiOiJqb2UiLA0KICJleHAiOjEzMDA4MTkzODAsDQogImh0dHA6Ly9leGFtcGxlLmNvbS9pc19yb290Ijp0cnVlfQ";
    const A2_N: &str = "ofgWCuLjybRlzo0tZWJjNiuSfb4p4fAkd_wWJcyQoTbji9k0l8W26mPddxHmfHQp-Vaw-4qPCJrcS2mJPMEzP1Pt0Bm4d4QlL-yRT-SFd2lZS-pCgNMsD1W_YpRPEwOWvG6b32690r2jZ47soMZo9wGzjb_7OMg0LOL-bSf63kpaSHSXndS5z5rexMdbBYUsLA9e-KXBdQOS-UTo7WTBEMa2R2CapHg665xsmtdVMTBQY4uDZlxvb3qCo5ZwKh9kG4LT6_I5IhlJH7aGhyxXFvUK-DWNmoudF8NAco9_h9iaGNj8q2ethFkMLs91kzk2PAcDTW9gb54h4FRWyuXpoQ";
    const A2_SIGNATURE: &str = "cC4hiUPoj9Eetdgtv3hF80EGrhuB__dzERat0XF9g2VtQgr9PJbu3XOiZj5RZmh7AAuHIm4Bh-0Qc_lF5YKt_O8W2Fp5jujGbds9uJdbF9CUAr7t1dnZcAcQjbKBYNX4BAynRFdiuB--f_nZLgrnbyTyWzO75vRK5h6xBArLIARNPvkSjtQBMHlb1L07Qe7K0GarZRmB_eSN9383LcOLn6_dO--xi12jzDwusC-eOkHWEsqtFZESc6BfI7noOPqvhJ1phCnvWh6IeYI2w9QOYEUipUTI8np6LbgGY9Fs98rqVt5AXLIhWkWywlVmtVrBp0igcN_IoypGlUPQGe77Rw";
    /// The same key as a `SubjectPublicKeyInfo` PEM, checked with `openssl rsa -pubin`.
    const A2_PEM: &str = "-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAofgWCuLjybRlzo0tZWJj
NiuSfb4p4fAkd/wWJcyQoTbji9k0l8W26mPddxHmfHQp+Vaw+4qPCJrcS2mJPMEz
P1Pt0Bm4d4QlL+yRT+SFd2lZS+pCgNMsD1W/YpRPEwOWvG6b32690r2jZ47soMZo
9wGzjb/7OMg0LOL+bSf63kpaSHSXndS5z5rexMdbBYUsLA9e+KXBdQOS+UTo7WTB
EMa2R2CapHg665xsmtdVMTBQY4uDZlxvb3qCo5ZwKh9kG4LT6/I5IhlJH7aGhyxX
FvUK+DWNmoudF8NAco9/h9iaGNj8q2ethFkMLs91kzk2PAcDTW9gb54h4FRWyuXp
oQIDAQAB
-----END PUBLIC KEY-----
";

    fn decoded(text: &str) -> Vec<u8> {
        URL_SAFE_NO_PAD.decode(text).expect("base64url")
    }

    fn a2_key() -> RsaVerifyingKey {
        RsaVerifyingKey::from_components(&decoded(A2_N), &EXPONENT).expect("the RFC's key")
    }

    fn a2_signature() -> RsaSignature {
        RsaSignature::from_slice(&decoded(A2_SIGNATURE)).expect("the RFC's signature")
    }

    #[test]
    fn the_rfc_7515_a2_signature_verifies() {
        assert!(
            a2_key()
                .verify(A2_INPUT.as_bytes(), &a2_signature())
                .is_ok()
        );
    }

    #[test]
    fn a_changed_message_or_signature_does_not_verify() {
        let key = a2_key();
        let mut input = A2_INPUT.as_bytes().to_vec();
        input[0] ^= 1;
        assert!(matches!(
            key.verify(&input, &a2_signature()),
            Err(KeyError::BadSignature)
        ));
        let mut bytes = decoded(A2_SIGNATURE);
        bytes[100] ^= 1;
        let altered = RsaSignature::from_slice(&bytes).expect("a length");
        assert!(matches!(
            key.verify(A2_INPUT.as_bytes(), &altered),
            Err(KeyError::BadSignature)
        ));
    }

    #[test]
    fn a_pem_reads_as_the_same_key() {
        let key = RsaVerifyingKey::from_public_key_pem(A2_PEM).expect("a key");
        assert_eq!(key, a2_key());
        assert!(key.verify(A2_INPUT.as_bytes(), &a2_signature()).is_ok());
    }

    #[test]
    fn a_weak_short_padded_or_unusual_key_is_refused() {
        let n = decoded(A2_N);
        let refused = |n: &[u8], e: &[u8]| {
            matches!(
                RsaVerifyingKey::from_components(n, e),
                Err(KeyError::Malformed(_))
            )
        };
        assert!(refused(&n, &[3]));
        assert!(refused(&n[..128], &EXPONENT));
        assert!(refused(&[&[0], n.as_slice()].concat(), &EXPONENT));
        assert!(refused(&[0xff; 513], &EXPONENT));
        assert!(!refused(&[0xff; 512], &EXPONENT));
    }

    #[test]
    fn a_pem_of_another_kind_is_refused() {
        let p256 = "-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAExYSh/SN98eeepxw7ihz7mV7YGg52
jG6VBagGEU7WC2b3fLA9XeButvVdZed1ug2OG7VsDwi2/IEWJ14SlpDAZQ==
-----END PUBLIC KEY-----
";
        for pem in [
            p256,
            "garbage",
            &A2_PEM.replace("PUBLIC KEY", "RSA PUBLIC KEY"),
        ] {
            assert!(matches!(
                RsaVerifyingKey::from_public_key_pem(pem),
                Err(KeyError::Malformed(_))
            ));
        }
    }

    #[test]
    fn its_debug_names_the_size_only() {
        assert_eq!(format!("{:?}", a2_key()), "RsaVerifyingKey(2048 bits)");
    }
}
