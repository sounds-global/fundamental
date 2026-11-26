use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use rustls_pki_types::CertificateDer;
use rustls_pki_types::pem::PemObject;

/// A certificate as PEM text, in exactly one shape: one `CERTIFICATE` block and nothing else.
///
/// A trust store installs every block in a file while a reader judges the first, so only
/// judged DER is ever written, and only one block is ever read.
#[derive(Debug, Clone, Copy)]
pub struct CertificatePem;

/// What opens any PEM block, whatever its label.
const BEGIN: &str = "-----BEGIN ";

impl CertificatePem {
    /// The one certificate `text` holds, or why it is not exactly one `CERTIFICATE` block.
    ///
    /// # Errors
    ///
    /// A sentence naming what `text` holds instead, for the caller's error.
    pub fn single(text: &str) -> Result<CertificateDer<'static>, String> {
        match text.matches(BEGIN).count() {
            0 => return Err("holds no PEM block".to_owned()),
            1 => {}
            blocks => {
                return Err(format!(
                    "holds {blocks} PEM blocks, and a root is exactly one certificate"
                ));
            }
        }
        CertificateDer::from_pem_slice(text.as_bytes())
            .map_err(|error| format!("holds no CERTIFICATE block ({error})"))
    }

    /// `der` as RFC 7468 PEM: one `CERTIFICATE` block, base64 in lines of 64.
    #[must_use]
    pub fn encode(der: &CertificateDer<'_>) -> String {
        let body = STANDARD.encode(der.as_ref());
        let mut pem = String::with_capacity(body.len() + body.len() / 64 + 64);
        pem.push_str("-----BEGIN CERTIFICATE-----\n");
        for line in body.as_bytes().chunks(64) {
            pem.push_str(&String::from_utf8_lossy(line));
            pem.push('\n');
        }
        pem.push_str("-----END CERTIFICATE-----\n");
        pem
    }
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use fundamental_core::address::TestApex;

    use super::CertificatePem;
    use crate::{ApexSet, ProjectName, ProjectRoot};

    fn root(name: &str, apex: &str) -> ProjectRoot {
        ProjectRoot::mint(
            &ProjectName::parse(name).expect("a name"),
            &ApexSet::new(TestApex::parse(apex)).expect("one"),
            OffsetDateTime::now_utc(),
        )
        .expect("a root")
    }

    #[test]
    fn the_encoding_is_what_rcgen_writes_and_reads_back() {
        let root = root("acme", "acme.test");
        let pem = CertificatePem::encode(root.der());
        assert_eq!(pem.replace("\r\n", "\n"), root.pem().replace("\r\n", "\n"));
        assert_eq!(CertificatePem::single(&pem).as_ref(), Ok(root.der()));
    }

    #[test]
    fn a_file_holding_a_second_block_is_refused() {
        let ours = root("acme", "acme.test");
        let theirs = root("evil", "evil.test");
        let appended = format!("{}{}", ours.pem(), theirs.pem());
        let prepended = format!("{}{}", theirs.pem(), ours.pem());
        let trusted = format!(
            "{}{}",
            ours.pem(),
            theirs
                .pem()
                .replace("BEGIN CERTIFICATE", "BEGIN TRUSTED CERTIFICATE")
                .replace("END CERTIFICATE", "END TRUSTED CERTIFICATE")
        );
        for text in [appended, prepended, trusted] {
            let refused = CertificatePem::single(&text).expect_err("two blocks");
            assert!(refused.contains("2 PEM blocks"), "{refused}");
        }
    }

    #[test]
    fn a_file_with_no_certificate_is_refused() {
        assert!(CertificatePem::single("").is_err());
        let key = "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n";
        let refused = CertificatePem::single(key).expect_err("not a certificate");
        assert!(refused.contains("no CERTIFICATE block"), "{refused}");
    }
}
