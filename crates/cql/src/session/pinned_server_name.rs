//! Verify a node's certificate against a configured name instead of the dialled address.

use std::sync::Arc;

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as TlsError, SignatureScheme};

/// A [`ServerCertVerifier`] that checks the chain exactly as rustls would, and the **name**
/// against a value from configuration rather than against the address the driver dialled.
///
/// The driver derives its `ServerName` from the dialled IP and offers no hook, so a privately
/// signed cluster would need an IP in every certificate. Chain, expiry and signatures stay
/// [`WebPkiServerVerifier`]'s. A host with a certificate for the pinned name from the anchor is
/// accepted at any address, so the anchor must be a private CA.
#[derive(Debug)]
pub(crate) struct PinnedServerName {
    name: ServerName<'static>,
    /// rustls' own verifier, which does all of the work except choosing the name.
    inner: Arc<WebPkiServerVerifier>,
}

impl PinnedServerName {
    pub(crate) const fn new(name: ServerName<'static>, inner: Arc<WebPkiServerVerifier>) -> Self {
        Self { name, inner }
    }
}

impl ServerCertVerifier for PinnedServerName {
    /// Delegates wholesale, with the configured name in place of the dialled one.
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _dialled: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        self.inner
            .verify_server_cert(end_entity, intermediates, &self.name, ocsp_response, now)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rustls::RootCertStore;
    use rustls::client::WebPkiServerVerifier;
    use rustls::client::danger::ServerCertVerifier;
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};

    use super::PinnedServerName;

    /// A throwaway ECDSA P-256 CA and two leaves it signed; their private keys were discarded.
    const TEST_CA: &str = "-----BEGIN CERTIFICATE-----
MIIBkjCCATmgAwIBAgIUQ8dqN6PFH5MO/aaaP16K03aLLIEwCgYIKoZIzj0EAwIw
HjEcMBoGA1UEAwwTZnVuZGFtZW50YWwgdGVzdCBDQTAgFw0yNjA4MDYxNzQwMDBa
GA8yMTI2MDcxMzE3NDAwMFowHjEcMBoGA1UEAwwTZnVuZGFtZW50YWwgdGVzdCBD
QTBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABOV5PV+vXBbmy37VX97+OWvxZZrV
zvpt1xcAPqrC9UdTzy/jDcdPZR5BR0oMv7JLYNL+OqflJmhdxXlvlPwuCaKjUzBR
MB0GA1UdDgQWBBRsYiHYk7zVaw0cnc2cYnBvnVDyPzAfBgNVHSMEGDAWgBRsYiHY
k7zVaw0cnc2cYnBvnVDyPzAPBgNVHRMBAf8EBTADAQH/MAoGCCqGSM49BAMCA0cA
MEQCIHD/7fZxKyGDP3LcLbUGGXedEHHHniDfB/BwC4LqIMFfAiBDYqSeyGjKR3VU
ktd9PKAf+NKKq1ZuBU0cXKjevMckog==
-----END CERTIFICATE-----
";

    /// `CN=scylla`, `subjectAltName=DNS:scylla`.
    const LEAF_CQL: &str = "-----BEGIN CERTIFICATE-----
MIIBqjCCAVGgAwIBAgIUEKnmXpMURdcjal3pg3XJ7NMiOrIwCgYIKoZIzj0EAwIw
HjEcMBoGA1UEAwwTZnVuZGFtZW50YWwgdGVzdCBDQTAgFw0yNjA4MDYxNzQwMTFa
GA8yMTI2MDcxMzE3NDAxMVowETEPMA0GA1UEAwwGc2N5bGxhMFkwEwYHKoZIzj0C
AQYIKoZIzj0DAQcDQgAEWyb9egoe/QLeSFQpIOtkjNyy6GYwsC+kG2km8pjzH0Bc
ykLHBUTIEY6idm9uWFkO+RTvcZOataKh2PoiQ/z1OqN4MHYwEQYDVR0RBAowCIIG
c2N5bGxhMBMGA1UdJQQMMAoGCCsGAQUFBwMBMAwGA1UdEwEB/wQCMAAwHQYDVR0O
BBYEFFp3RcjYKOfqPYQS4qn6Vxl19YKSMB8GA1UdIwQYMBaAFGxiIdiTvNVrDRyd
zZxicG+dUPI/MAoGCCqGSM49BAMCA0cAMEQCIGlv6NceXIkMsvnBFtKsBf5GtKiG
t816L5iT3HFSy7WqAiBPqWGoBRsBAg1D35Aj08K3CyRvDxemIocESDIqC8j4Ew==
-----END CERTIFICATE-----
";

    /// `CN=other`, `subjectAltName=DNS:other` — same CA, wrong name.
    const LEAF_OTHER: &str = "-----BEGIN CERTIFICATE-----
MIIBqjCCAU+gAwIBAgIUEKnmXpMURdcjal3pg3XJ7NMiOrMwCgYIKoZIzj0EAwIw
HjEcMBoGA1UEAwwTZnVuZGFtZW50YWwgdGVzdCBDQTAgFw0yNjA4MDYxNzQwMTFa
GA8yMTI2MDcxMzE3NDAxMVowEDEOMAwGA1UEAwwFb3RoZXIwWTATBgcqhkjOPQIB
BggqhkjOPQMBBwNCAASFr5pNL5qod8sV5LG/64HnpcQ6TdMYSd61ZULlLi/XYYWV
LdCk84EYMUpsW/r1SchNZG5UAtG8KJBzPSgug8Koo3cwdTAQBgNVHREECTAHggVv
dGhlcjATBgNVHSUEDDAKBggrBgEFBQcDATAMBgNVHRMBAf8EAjAAMB0GA1UdDgQW
BBTK2fQR0jNVvF5oycisZv1wem6RmTAfBgNVHSMEGDAWgBRsYiHYk7zVaw0cnc2c
YnBvnVDyPzAKBggqhkjOPQQDAgNJADBGAiEAuFVzoTJ/36AMVyEeeljdSOZ+K0jC
pY4vEfEquUW8PrcCIQCOnQt4rTvHmAXOaNXCby+1DetAM2jx47WgfAfi5gm+iw==
-----END CERTIFICATE-----
";

    /// A moment inside every fixture's validity, fixed so these tests never become expiry tests.
    fn during_validity() -> UnixTime {
        // 2030-01-01T00:00:00Z — after the 2026 notBefore, long before the 2126 notAfter.
        UnixTime::since_unix_epoch(std::time::Duration::from_secs(1_893_456_000))
    }

    fn der(pem: &str) -> CertificateDer<'static> {
        CertificateDer::pem_slice_iter(pem.as_bytes())
            .next()
            .expect("the fixture holds one certificate")
            .expect("the fixture parses")
    }

    /// Matched against a baked fixture whose SAN is `DNS:scylla`.
    fn verifier_pinned_to(name: &'static str) -> PinnedServerName {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let mut roots = RootCertStore::empty();
        roots.add(der(TEST_CA)).expect("the test CA is an anchor");
        let inner = WebPkiServerVerifier::builder(Arc::new(roots))
            .build()
            .expect("a verifier over one anchor");
        PinnedServerName::new(ServerName::try_from(name).expect("a valid DNS name"), inner)
    }

    #[test]
    fn a_certificate_for_the_pinned_name_is_accepted_at_any_address() {
        let dialled = ServerName::try_from("192.168.97.6").expect("an IP is a server name");
        verifier_pinned_to("scylla")
            .verify_server_cert(&der(LEAF_CQL), &[], &dialled, &[], during_validity())
            .expect("the pinned name matches, so the chain and name both verify");
    }

    /// If this ever passes, the override has stopped checking names at all.
    #[test]
    fn a_certificate_for_another_name_is_refused_even_from_the_same_ca() {
        let dialled = ServerName::try_from("192.168.97.6").expect("an IP is a server name");
        let error = verifier_pinned_to("scylla")
            .verify_server_cert(&der(LEAF_OTHER), &[], &dialled, &[], during_validity())
            .expect_err("`other` is not `scylla`");
        assert!(
            matches!(error, rustls::Error::InvalidCertificate(_)),
            "expected a certificate rejection, got {error:?}"
        );
    }

    #[test]
    fn the_chain_is_still_verified_to_the_configured_anchor() {
        let dialled = ServerName::try_from("cql").expect("a valid DNS name");
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let mut roots = RootCertStore::empty();
        // An anchor that signed neither leaf.
        roots.add(der(LEAF_OTHER)).expect("parsed as an anchor");
        let inner = WebPkiServerVerifier::builder(Arc::new(roots))
            .build()
            .expect("a verifier over one anchor");
        let verifier = PinnedServerName::new(ServerName::try_from("cql").expect("valid"), inner);
        verifier
            .verify_server_cert(&der(LEAF_CQL), &[], &dialled, &[], during_validity())
            .expect_err("the leaf does not chain to that anchor");
    }

    #[test]
    fn the_supported_schemes_are_the_delegates() {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let mut roots = RootCertStore::empty();
        roots.add(der(TEST_CA)).expect("the test CA is an anchor");
        let inner = WebPkiServerVerifier::builder(Arc::new(roots))
            .build()
            .expect("a verifier over one anchor");
        let expected = inner.supported_verify_schemes();
        let verifier = PinnedServerName::new(ServerName::try_from("cql").expect("valid"), inner);
        assert_eq!(verifier.supported_verify_schemes(), expected);
    }
}
