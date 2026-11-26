use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use rcgen::{
    BasicConstraints, CertificateParams, CidrSubnet, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, GeneralSubtree, IsCa, Issuer, KeyPair, KeyUsagePurpose,
    NameConstraints, SerialNumber,
};
use ring::rand::{SecureRandom, SystemRandom};
use rustls_pki_types::CertificateDer;
use time::{Duration, OffsetDateTime};
use tracing::{info, instrument};

use crate::{ApexSet, CertificatePem, LocalError, LocalState, ProjectName, RootFingerprint};

/// How long a project's root is valid: it is trusted by hand, so it outlives many leaves.
const ROOT_LIFETIME: Duration = Duration::days(3650);

/// A project's development root: an ECDSA P-256 CA limited by a critical name constraint to
/// server certificates, one level deep, for the project's own apexes and never an IP address.
#[derive(Debug)]
pub struct ProjectRoot {
    der: CertificateDer<'static>,
    pem: String,
    key: KeyPair,
    fingerprint: RootFingerprint,
}

impl ProjectRoot {
    /// Mints a root for `project`, constrained to `apexes`.
    ///
    /// # Errors
    ///
    /// [`LocalError::LeafCoversTooMuch`] (`LOCAL_006`, probe `mint`) when the key or certificate cannot be made.
    #[instrument(skip_all, fields(project = %project, apexes = %apexes))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn mint(
        project: &ProjectName,
        apexes: &ApexSet,
        now: OffsetDateTime,
    ) -> Result<Self, LocalError> {
        let key = KeyPair::generate().map_err(|error| failed("mint", &error))?;
        let params = Self::params(project, apexes, now)?;
        let certificate = params
            .self_signed(&key)
            .map_err(|error| failed("mint", &error))?;
        let root = Self::from_parts(certificate.der().clone(), certificate.pem(), key);
        info!(project = %project, fingerprint = %root.fingerprint, "local.root_minted");
        Ok(root)
    }

    fn params(
        project: &ProjectName,
        apexes: &ApexSet,
        now: OffsetDateTime,
    ) -> Result<CertificateParams, LocalError> {
        let mut params = CertificateParams::default();
        let mut name = DistinguishedName::new();
        name.push(DnType::OrganizationName, "Fundamental local development");
        name.push(DnType::CommonName, format!("fundamental local {project}"));
        params.distinguished_name = name;
        params.serial_number = Some(serial()?);
        params.not_before = now - Duration::hours(1);
        params.not_after = now + ROOT_LIFETIME;
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.name_constraints = Some(NameConstraints {
            permitted_subtrees: apexes
                .permitted()
                .into_iter()
                .map(GeneralSubtree::DnsName)
                .collect(),
            excluded_subtrees: vec![
                GeneralSubtree::IpAddress(CidrSubnet::V4(Ipv4Addr::UNSPECIFIED.octets(), [0; 4])),
                GeneralSubtree::IpAddress(CidrSubnet::V6(Ipv6Addr::UNSPECIFIED.octets(), [0; 16])),
            ],
        });
        Ok(params)
    }

    fn from_parts(der: CertificateDer<'static>, pem: String, key: KeyPair) -> Self {
        let fingerprint = RootFingerprint::of_der(&der);
        Self {
            der,
            pem,
            key,
            fingerprint,
        }
    }

    /// The root's fingerprint.
    #[must_use]
    pub const fn fingerprint(&self) -> RootFingerprint {
        self.fingerprint
    }

    /// The certificate's DER bytes.
    #[must_use]
    pub const fn der(&self) -> &CertificateDer<'static> {
        &self.der
    }

    /// The certificate as PEM: what a trust store is given.
    #[must_use]
    pub fn pem(&self) -> &str {
        &self.pem
    }

    /// The certificate file of the root with `fingerprint` under `state`.
    #[must_use]
    pub fn certificate_path(state: &LocalState, fingerprint: &RootFingerprint) -> PathBuf {
        state.root_dir(fingerprint).join("root.pem")
    }

    fn key_path(state: &LocalState, fingerprint: &RootFingerprint) -> PathBuf {
        state.root_dir(fingerprint).join("key.pem")
    }

    /// Writes the root under its fingerprint: the certificate, and the key at mode `0600`.
    ///
    /// Each file is created new, so a root a browser trusts is never overwritten.
    ///
    /// # Errors
    ///
    /// [`LocalError::Io`] when a directory or file cannot be created, or already exists.
    pub fn store(&self, state: &LocalState) -> Result<(), LocalError> {
        let dir = state.root_dir(&self.fingerprint);
        state.private_dir(&dir)?;
        write_new(
            &Self::certificate_path(state, &self.fingerprint),
            self.pem.as_bytes(),
            0o644,
        )?;
        write_new(
            &Self::key_path(state, &self.fingerprint),
            self.key.serialize_pem().as_bytes(),
            0o600,
        )
    }

    /// Reads the root with `fingerprint` back from `state`, checking its bytes are that root.
    ///
    /// # Errors
    ///
    /// [`LocalError::RootMissing`] (`LOCAL_016`) when a file is gone or not the registered root;
    /// [`LocalError::LeafCoversTooMuch`] (`LOCAL_006`) when it does not parse or holds more than one certificate.
    pub fn load(
        state: &LocalState,
        project: &ProjectName,
        fingerprint: &RootFingerprint,
    ) -> Result<Self, LocalError> {
        let certificate = Self::certificate_path(state, fingerprint);
        let missing = || LocalError::RootMissing {
            project: project.to_string(),
            fingerprint: fingerprint.to_string(),
            path: state.root_dir(fingerprint).display().to_string(),
        };
        let pem = match fs::read_to_string(&certificate) {
            Ok(pem) => pem,
            Err(error) if error.kind() == ErrorKind::NotFound => return Err(missing()),
            Err(error) => return Err(LocalError::io(&certificate, "read", error)),
        };
        let key_path = Self::key_path(state, fingerprint);
        let key_pem = match fs::read_to_string(&key_path) {
            Ok(pem) => pem,
            Err(error) if error.kind() == ErrorKind::NotFound => return Err(missing()),
            Err(error) => return Err(LocalError::io(&key_path, "read", error)),
        };
        let der = CertificatePem::single(&pem).map_err(|detail| LocalError::LeafCoversTooMuch {
            probe: "load".to_owned(),
            detail: format!("{} {detail}", certificate.display()),
        })?;
        let key = KeyPair::from_pem(&key_pem).map_err(|error| failed("load", &error))?;
        let root = Self::from_parts(der, pem, key);
        if root.fingerprint == *fingerprint {
            Ok(root)
        } else {
            Err(missing())
        }
    }

    /// The issuer leaves are signed by: this root's own subject and key.
    ///
    /// # Errors
    ///
    /// [`LocalError::LeafCoversTooMuch`] (`LOCAL_006`) when the root's bytes do not parse.
    pub fn issuer(&self) -> Result<Issuer<'static, &KeyPair>, LocalError> {
        Issuer::from_ca_cert_der(&self.der, &self.key).map_err(|error| failed("issuer", &error))
    }
}

/// A fresh serial: 16 bytes from the operating system's generator, positive.
pub(crate) fn serial() -> Result<SerialNumber, LocalError> {
    let mut bytes = [0_u8; 16];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| LocalError::LeafCoversTooMuch {
            probe: "serial".to_owned(),
            detail: "the operating system's random generator failed".to_owned(),
        })?;
    bytes[0] &= 0x7f;
    Ok(SerialNumber::from_slice(&bytes))
}

pub(crate) fn failed(probe: &str, error: &rcgen::Error) -> LocalError {
    LocalError::LeafCoversTooMuch {
        probe: probe.to_owned(),
        detail: error.to_string(),
    }
}

fn write_new(path: &Path, bytes: &[u8], mode: u32) -> Result<(), LocalError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .map_err(|error| LocalError::io(path, "created", error))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| LocalError::io(path, "written", error))
}
