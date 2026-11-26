use std::fmt;
use std::sync::Arc;

use arc_swap::ArcSwapOption;
use fundamental_keys::{DeviceKey, KeyCustody, certificate_key};
use rustls::client::ResolvesClientCert;
use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, SubjectPublicKeyInfoDer};
use rustls::sign::{CertifiedKey, Signer, SigningKey};
use rustls::{SignatureAlgorithm, SignatureScheme};
use tracing::{error, info, instrument, warn};

use crate::NativeError;

/// The one scheme a P-256 [`DeviceKey`] signs with.
const SCHEME: SignatureScheme = SignatureScheme::ECDSA_NISTP256_SHA256;

/// A device's client identity: a [`DeviceKey`] the core process holds, and the certificate
/// chain a certificate authority issued to it.
///
/// Presented by [`RemoteBridge::with_client_identity`](crate::RemoteBridge::with_client_identity);
/// each handshake asks the key to sign. [`install`](Self::install) takes effect on the next
/// connection; until then nothing is presented.
pub struct ClientIdentity {
    key: Arc<dyn DeviceKey>,
    certified: ArcSwapOption<CertifiedKey>,
}

impl ClientIdentity {
    /// An identity over `key`, with no certificate yet.
    #[must_use]
    pub fn new(key: impl DeviceKey + 'static) -> Self {
        Self {
            key: Arc::new(key),
            certified: ArcSwapOption::empty(),
        }
    }

    /// The device's key, which a certificate request is signed with.
    #[must_use]
    pub fn key(&self) -> &dyn DeviceKey {
        self.key.as_ref()
    }

    /// Replace the presented chain with `chain_pem`: the leaf first, then any intermediates.
    ///
    /// # Errors
    /// [`NativeError::ClientCertificate`] when `chain_pem` holds no certificate, does not parse,
    /// or its leaf names another key; the chain in use is then kept.
    #[instrument(skip_all, fields(custody = ?self.key.custody()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn install(&self, chain_pem: &str) -> Result<(), NativeError> {
        let refused = |detail: String| NativeError::ClientCertificate { detail };
        let chain = CertificateDer::pem_slice_iter(chain_pem.as_bytes())
            .collect::<Result<Vec<CertificateDer<'static>>, _>>()
            .map_err(|error| refused(format!("the chain does not parse as PEM: {error}")))?;
        let Some(leaf) = chain.first() else {
            return Err(refused("the chain holds no certificate".to_owned()));
        };
        let issued_to = certificate_key(leaf).map_err(|error| refused(error.to_string()))?;
        if issued_to != self.key.public_key_der() {
            return Err(refused(
                "the leaf certificate names a different key than this device's".to_owned(),
            ));
        }
        let length = chain.len();
        let signer = Arc::new(DeviceSigningKey(Arc::clone(&self.key)));
        self.certified
            .store(Some(Arc::new(CertifiedKey::new(chain, signer))));
        info!(chain_length = length, "client certificate installed");
        Ok(())
    }

    /// Whether a chain is installed, and so presented on the next connection.
    #[must_use]
    pub fn has_certificate(&self) -> bool {
        self.certified.load().is_some()
    }

    /// Stops presenting any chain.
    pub fn clear(&self) {
        self.certified.store(None);
        info!("client certificate cleared");
    }
}

impl fmt::Debug for ClientIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientIdentity")
            .field("custody", &self.key.custody())
            .field("has_certificate", &self.has_certificate())
            .finish_non_exhaustive()
    }
}

impl ResolvesClientCert for ClientIdentity {
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn resolve(
        &self,
        _root_hint_subjects: &[&[u8]],
        sigschemes: &[SignatureScheme],
    ) -> Option<Arc<CertifiedKey>> {
        let certified = self.certified.load_full();
        if certified.is_none() {
            warn!("the server asked for a client certificate and none is installed");
            return None;
        }
        if !sigschemes.contains(&SCHEME) {
            warn!(
                offered = ?sigschemes,
                "the server accepts no signature scheme a P-256 device key makes"
            );
            return None;
        }
        certified
    }

    fn has_certs(&self) -> bool {
        self.has_certificate()
    }
}

/// A [`DeviceKey`] in the shape rustls signs a handshake with.
#[derive(Debug)]
struct DeviceSigningKey(Arc<dyn DeviceKey>);

impl SigningKey for DeviceSigningKey {
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn Signer>> {
        offered
            .contains(&SCHEME)
            .then(|| Box::new(DeviceSigner(Arc::clone(&self.0))) as Box<dyn Signer>)
    }

    fn public_key(&self) -> Option<SubjectPublicKeyInfoDer<'_>> {
        Some(SubjectPublicKeyInfoDer::from(self.0.public_key_der()))
    }

    fn algorithm(&self) -> SignatureAlgorithm {
        SignatureAlgorithm::ECDSA
    }
}

#[derive(Debug)]
struct DeviceSigner(Arc<dyn DeviceKey>);

impl Signer for DeviceSigner {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rustls::Error> {
        self.0.sign(message).map_err(|failure| {
            let custody: KeyCustody = self.0.custody();
            error!(?custody, error = %failure, "the device key refused to sign a handshake");
            rustls::Error::General(format!("the device key did not sign: {failure}"))
        })
    }

    fn scheme(&self) -> SignatureScheme {
        SCHEME
    }
}
