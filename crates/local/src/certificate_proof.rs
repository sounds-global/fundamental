use std::net::IpAddr;

use rcgen::{CertificateParams, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, SanType};
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use time::{Duration, OffsetDateTime};
use tracing::{error, info};
use webpki::{EndEntityCert, KeyUsage, anchor_from_trusted_cert};

use crate::project_root::{failed, serial};
use crate::{LocalError, ProjectRoot};

/// What a root or a leaf must and must not verify for, checked with rustls-webpki as a TLS
/// client checks a server.
#[derive(Debug, Clone, Copy)]
pub struct CertificateProof {
    now: OffsetDateTime,
}

impl CertificateProof {
    /// Proofs judged at `now`.
    #[must_use]
    pub const fn at(now: OffsetDateTime) -> Self {
        Self { now }
    }

    /// Proves `root` permits what it was minted for and nothing else: a throwaway leaf verifies
    /// for `inside` and is refused for each of `outside`.
    ///
    /// # Errors
    ///
    /// [`LocalError::LeafCoversTooMuch`] (`LOCAL_006`) naming the probe that answered wrongly.
    pub fn root(
        &self,
        root: &ProjectRoot,
        inside: &str,
        outside: &[Probe],
    ) -> Result<(), LocalError> {
        let accepted = self.throwaway(root, &Probe::Name(inside.to_owned()))?;
        self.expect(root.der(), &accepted, inside, true, "root-permits-its-own")?;
        for probe in outside {
            let leaf = self.throwaway(root, probe)?;
            self.expect(
                root.der(),
                &leaf,
                &probe.subject(),
                false,
                "root-refuses-outside",
            )?;
        }
        Ok(())
    }

    /// Proves `leaf` chains to `root` and is valid for each of `own` and none of `refused`.
    ///
    /// # Errors
    ///
    /// [`LocalError::LeafCoversTooMuch`] (`LOCAL_006`) naming the probe that answered wrongly.
    pub fn leaf(
        &self,
        root: &CertificateDer<'_>,
        leaf: &CertificateDer<'_>,
        own: &[String],
        refused: &[String],
    ) -> Result<(), LocalError> {
        for name in own {
            self.expect(root, leaf, name, true, "leaf-covers-its-own")?;
        }
        for name in refused {
            self.expect(root, leaf, name, false, "leaf-refuses-a-neighbour")?;
        }
        Ok(())
    }

    /// Whether `leaf` verifies against `root` for `subject` right now.
    #[must_use]
    pub fn verifies(
        &self,
        root: &CertificateDer<'_>,
        leaf: &CertificateDer<'_>,
        subject: &str,
    ) -> bool {
        self.verdict(root, leaf, subject).is_ok()
    }

    #[expect(
        clippy::cognitive_complexity,
        clippy::too_many_arguments,
        reason = "readability baseline"
    )]
    fn expect(
        &self,
        root: &CertificateDer<'_>,
        leaf: &CertificateDer<'_>,
        subject: &str,
        accepted: bool,
        probe: &str,
    ) -> Result<(), LocalError> {
        let verdict = self.verdict(root, leaf, subject);
        if verdict.is_ok() == accepted {
            info!(probe, subject, outcome = "as-expected", "local.proof");
            return Ok(());
        }
        let detail = match verdict {
            Ok(()) => format!("`{subject}` verified, and must not"),
            Err(why) => format!("`{subject}` did not verify, and must: {why}"),
        };
        error!(probe, subject, outcome = "wrong", %detail, error_code = "LOCAL_006", "local.proof");
        Err(LocalError::LeafCoversTooMuch {
            probe: probe.to_owned(),
            detail,
        })
    }

    fn verdict(
        &self,
        root: &CertificateDer<'_>,
        leaf: &CertificateDer<'_>,
        subject: &str,
    ) -> Result<(), String> {
        let anchor = anchor_from_trusted_cert(root).map_err(|error| error.to_string())?;
        let end_entity = EndEntityCert::try_from(leaf).map_err(|error| error.to_string())?;
        let seconds =
            u64::try_from(self.now.unix_timestamp()).map_err(|error| error.to_string())?;
        end_entity
            .verify_for_usage(
                &[webpki::aws_lc_rs::ECDSA_P256_SHA256],
                &[anchor],
                &[],
                UnixTime::since_unix_epoch(std::time::Duration::from_secs(seconds)),
                KeyUsage::server_auth(),
                None,
                None,
            )
            .map_err(|error| error.to_string())?;
        let name = ServerName::try_from(subject).map_err(|error| error.to_string())?;
        end_entity
            .verify_is_valid_for_subject_name(&name)
            .map_err(|error| error.to_string())
    }

    fn throwaway(
        &self,
        root: &ProjectRoot,
        probe: &Probe,
    ) -> Result<CertificateDer<'static>, LocalError> {
        let mut params = CertificateParams::default();
        params.subject_alt_names = vec![probe.san()?];
        params.is_ca = IsCa::ExplicitNoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.serial_number = Some(serial()?);
        params.not_before = self.now - Duration::hours(1);
        params.not_after = self.now + Duration::hours(1);
        let key = KeyPair::generate().map_err(|error| failed("probe", &error))?;
        let issuer = root.issuer()?;
        params
            .signed_by(&key, &issuer)
            .map(|certificate| certificate.der().clone())
            .map_err(|error| failed("probe", &error))
    }
}

/// A name a throwaway leaf is minted for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// A DNS name.
    Name(String),
    /// An IP address.
    Address(IpAddr),
}

impl Probe {
    fn san(&self) -> Result<SanType, LocalError> {
        match self {
            Self::Name(name) => name
                .clone()
                .try_into()
                .map(SanType::DnsName)
                .map_err(|error| failed("probe", &error)),
            Self::Address(address) => Ok(SanType::IpAddress(*address)),
        }
    }

    fn subject(&self) -> String {
        match self {
            Self::Name(name) => name.clone(),
            Self::Address(address) => address.to_string(),
        }
    }
}
