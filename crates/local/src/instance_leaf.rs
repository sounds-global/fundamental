use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use fundamental_core::fs::replace_file;
use fundamental_core::secrets::{InMemorySecret, InMemorySecretString};
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose, SanType,
};
use rustls_pki_types::CertificateDer;
use rustls_pki_types::pem::PemObject;
use time::{Duration, OffsetDateTime};
use tracing::{info, instrument, warn};
use x509_parser::prelude::{FromDer, GeneralName, X509Certificate};

use crate::project_root::{failed, serial};
use crate::{CertificateProof, LocalError, ProjectRoot};

/// How long an instance's leaf is valid: the 397 days a public CA may issue for.
const LEAF_LIFETIME: Duration = Duration::days(397);
/// A leaf with less left than this is issued again by the next `up`.
const RENEW_BEFORE: Duration = Duration::days(30);

/// One instance's server certificate and key, written together as `tls.pem`.
#[derive(Debug, Clone)]
pub struct InstanceLeaf {
    der: CertificateDer<'static>,
    certificate: String,
    key: InMemorySecretString,
}

impl InstanceLeaf {
    /// Issues a leaf signed by `root` for exactly `names`.
    ///
    /// # Errors
    ///
    /// [`LocalError::LeafCoversTooMuch`] (`LOCAL_006`) when a name cannot be written into a
    /// certificate or the leaf cannot be signed.
    #[instrument(skip_all, fields(names = names.len()))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn issue(
        root: &ProjectRoot,
        names: &[String],
        now: OffsetDateTime,
    ) -> Result<Self, LocalError> {
        let mut params = CertificateParams::default();
        let mut subject = DistinguishedName::new();
        subject.push(
            DnType::CommonName,
            names.first().cloned().unwrap_or_default(),
        );
        params.distinguished_name = subject;
        params.subject_alt_names = names
            .iter()
            .map(|name| {
                name.clone()
                    .try_into()
                    .map(SanType::DnsName)
                    .map_err(|error| failed("issue", &error))
            })
            .collect::<Result<_, _>>()?;
        params.is_ca = IsCa::ExplicitNoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.use_authority_key_identifier_extension = true;
        let serial = serial()?;
        params.serial_number = Some(serial.clone());
        params.not_before = now - Duration::hours(1);
        params.not_after = now + LEAF_LIFETIME;
        let key = KeyPair::generate().map_err(|error| failed("issue", &error))?;
        let certificate = params
            .signed_by(&key, &root.issuer()?)
            .map_err(|error| failed("issue", &error))?;
        info!(
            names = %names.join(","),
            serial = %serial,
            not_after = %params.not_after,
            "local.leaf_issued"
        );
        Ok(Self {
            der: certificate.der().clone(),
            certificate: certificate.pem(),
            key: InMemorySecret::new(key.serialize_pem()),
        })
    }

    /// The certificate's DER bytes.
    #[must_use]
    pub const fn der(&self) -> &CertificateDer<'static> {
        &self.der
    }

    /// Writes the certificate and its key to `path` in one atomic replace, mode `0600`.
    ///
    /// # Errors
    ///
    /// [`LocalError::Io`] when the file cannot be written; [`LocalError::LeafCoversTooMuch`]
    /// (`LOCAL_006`, probe `write`) when the key has already been cleared.
    pub fn write(&self, path: &Path) -> Result<(), LocalError> {
        let key = self
            .key
            .expose()
            .ok_or_else(|| LocalError::LeafCoversTooMuch {
                probe: "write".to_owned(),
                detail: "the leaf's key was cleared before it was written".to_owned(),
            })?;
        replace_file(path, format!("{}{key}", self.certificate).as_bytes())
            .map_err(|error| LocalError::io(path, "written", error))?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| LocalError::io(path, "made private", error))
    }

    /// Whether the leaf at `path` can stay: exactly `names`, chained to `root`, thirty days left
    /// at `now`; anything unreadable is `false`.
    #[must_use]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn still_serves(
        path: &Path,
        root: &ProjectRoot,
        names: &[String],
        now: OffsetDateTime,
    ) -> bool {
        let pem = match fs::read_to_string(path) {
            Ok(pem) => pem,
            Err(error) if error.kind() == ErrorKind::NotFound => return false,
            Err(error) => {
                warn!(path = %path.display(), %error, "the leaf could not be read; issuing another");
                return false;
            }
        };
        let der = match CertificateDer::from_pem_slice(pem.as_bytes()) {
            Ok(der) => der,
            Err(error) => {
                warn!(path = %path.display(), %error, "the leaf holds no certificate; issuing another");
                return false;
            }
        };
        let certificate = match X509Certificate::from_der(der.as_ref()) {
            Ok((_, certificate)) => certificate,
            Err(error) => {
                warn!(path = %path.display(), %error, "the leaf's certificate does not parse; issuing another");
                return false;
            }
        };
        let mut written: Vec<String> = certificate
            .subject_alternative_name()
            .ok()
            .flatten()
            .map(|extension| {
                extension
                    .value
                    .general_names
                    .iter()
                    .filter_map(|name| match name {
                        GeneralName::DNSName(name) => Some((*name).to_owned()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut wanted = names.to_vec();
        written.sort();
        wanted.sort();
        let left = certificate.validity().not_after.timestamp() - now.unix_timestamp();
        let proof = CertificateProof::at(now);
        written == wanted
            && left > RENEW_BEFORE.whole_seconds()
            && names
                .first()
                .is_some_and(|name| proof.verifies(root.der(), &der, &name.replace('*', "probe")))
    }
}
