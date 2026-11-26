use crate::{ServerError, ServerResult};

/// A `'sha256-…'` source: one exact inline script or `<style>`, admitted by the digest of its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CspHash {
    base64: String,
}

impl CspHash {
    /// A SHA-256 hash source from its standard, padded base64 encoding.
    ///
    /// # Errors
    /// [`ServerError::SecurityHeaderInvalid`] (`API_018`) when `base64` is not the
    /// 44-character standard encoding of 32 bytes.
    pub fn sha256_base64(base64: &str) -> ServerResult<Self> {
        // The last character carries two zero bits in canonical base64; other spellings differ between browsers.
        let well_formed = base64.len() == 44
            && base64.ends_with('=')
            && !base64[..43].contains('=')
            && base64
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
            && base64
                .as_bytes()
                .get(42)
                .is_some_and(|last| b"AEIMQUYcgkosw048".contains(last));
        if !well_formed {
            return Err(ServerError::SecurityHeaderInvalid {
                header: "content-security-policy",
                reason: "a script hash is not the base64 of a SHA-256 digest",
            });
        }
        Ok(Self {
            base64: base64.to_owned(),
        })
    }

    /// The source expression, quotes included: `'sha256-…'`.
    #[must_use]
    pub fn source(&self) -> String {
        format!("'sha256-{}'", self.base64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    #[test]
    fn a_digest_is_admitted_as_a_quoted_source() {
        let hash = CspHash::sha256_base64("47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=")
            .expect("the digest of the empty string");
        assert_eq!(
            hash.source(),
            "'sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU='"
        );
    }

    #[test]
    fn anything_else_is_api_018() {
        for bad in [
            "",
            "short=",
            "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU",
            "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSu'=",
            "'unsafe-inline' 47DEQpj8HBSa+/TImW+5JCeuQ=",
            "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFV=",
        ] {
            let error = CspHash::sha256_base64(bad).expect_err(bad);
            assert_eq!(error.error_code(), "API_018", "{bad}");
        }
    }
}
