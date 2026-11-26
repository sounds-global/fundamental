use der::{Decode as _, Encode as _};
use x509_cert::Certificate;

use crate::KeyError;

/// The DER `SubjectPublicKeyInfo` that the DER certificate `certificate` was issued to.
///
/// Compared with [`DeviceKey::public_key_der`](super::DeviceKey::public_key_der) before a
/// certificate is installed.
///
/// # Errors
/// [`KeyError::Malformed`] when `certificate` is not a DER X.509 certificate.
pub fn certificate_key(certificate: &[u8]) -> Result<Vec<u8>, KeyError> {
    Certificate::from_der(certificate)
        .and_then(|parsed| parsed.tbs_certificate.subject_public_key_info.to_der())
        .map_err(|error| KeyError::Malformed(format!("not an X.509 certificate: {error}")))
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::certificate_key;
    use crate::KeyError;

    #[test]
    fn bytes_that_are_not_a_certificate_are_malformed() {
        let error = certificate_key(b"not a certificate").expect_err("refused");
        assert!(matches!(error, KeyError::Malformed(_)));
        assert_eq!(error.error_code(), "KEYS_003");
    }
}
