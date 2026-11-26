use std::str::FromStr as _;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::thread;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use fundamental_core::secrets::SecretString;
use p256::elliptic_curve::sec1::FromEncodedPoint as _;
use p256::pkcs8::EncodePublicKey as _;
use p256::{EncodedPoint, FieldBytes, PublicKey};
use sha2::{Digest as _, Sha256};
use tracing::{debug, error, info, warn};
use tss_esapi::Context;
use tss_esapi::attributes::ObjectAttributesBuilder;
use tss_esapi::constants::tss::{TPM2_RH_NULL, TPM2_ST_HASHCHECK};
use tss_esapi::handles::KeyHandle;
use tss_esapi::interface_types::algorithm::{HashingAlgorithm, PublicAlgorithm};
use tss_esapi::interface_types::ecc::EccCurve;
use tss_esapi::interface_types::resource_handles::Hierarchy;
use tss_esapi::structures::{
    Digest, EccPoint, EccScheme, HashScheme, HashcheckTicket, Private, Public, PublicBuilder,
    PublicEccParametersBuilder, Signature, SignatureScheme, SymmetricDefinitionObject,
};
use tss_esapi::tcti_ldr::TctiNameConf;
use tss_esapi::traits::{Marshall as _, UnMarshall as _};
use tss_esapi::tss2_esys::TPMT_TK_HASHCHECK;
use tss_esapi::utils::create_unrestricted_signing_ecc_public;

use super::{DeviceKey, KeyCustody};
use crate::{KeyError, SecretStore};

const STORED: &str = "tpm2-p256:v1";

/// A P-256 key the machine's TPM generated, under a storage key it derives from its own seed.
///
/// The private half never leaves the TPM; the OS key store keeps the wrapped blob. A TPM context
/// is confined to its thread, so one thread owns it and [`sign`](DeviceKey::sign) messages it.
pub struct TpmDeviceKey {
    public: Vec<u8>,
    jobs: Mutex<Sender<SignJob>>,
}

struct SignJob {
    digest: [u8; 32],
    reply: SyncSender<Result<Vec<u8>, KeyError>>,
}

impl TpmDeviceKey {
    /// The key `store` keeps under `account` for the TPM `tcti` names (`device:/dev/tpmrm0` for
    /// the machine's own), or a new one generated there.
    ///
    /// # Errors
    /// [`KeyError::Custody`] when the TPM cannot be reached, refuses, or does not hold the
    /// storage key the stored blob was wrapped under — another machine's key; [`KeyError::Malformed`]
    /// when what `store` holds is not a TPM key.
    pub fn open_or_create(
        tcti: &str,
        store: &dyn SecretStore,
        account: &str,
    ) -> Result<Self, KeyError> {
        let tcti =
            TctiNameConf::from_str(tcti).map_err(|error| custody("the TPM's address", &error))?;
        let stored = store
            .load(account)?
            .map(|kept| decode(kept.expose()))
            .transpose()?;
        let (ready, readied) = sync_channel(1);
        let (jobs, received) = channel::<SignJob>();
        thread::Builder::new()
            .name("tpm-device-key".to_owned())
            .spawn(move || match Loaded::open(tcti, stored) {
                Ok(loaded) => {
                    let opened = (loaded.public.clone(), loaded.created.clone());
                    if ready.send(Ok(opened)).is_ok() {
                        loaded.serve(&received);
                    }
                }
                Err(failure) => {
                    if let Err(unsent) = ready.send(Err(failure)) {
                        error!(failure = ?unsent.0, "the TPM key did not open, and nobody was waiting to hear it");
                    }
                }
            })
            .map_err(|error| KeyError::Custody(format!("the TPM's thread did not start: {error}")))?;
        let (public, created) = readied.recv().map_err(|_| {
            KeyError::Custody("the TPM's thread ended before it answered".to_owned())
        })??;
        if let Some(blob) = created {
            store.store(account, &SecretString::new(blob))?;
            info!("generated a device key in the TPM");
        }
        Ok(Self {
            public,
            jobs: Mutex::new(jobs),
        })
    }
}

impl DeviceKey for TpmDeviceKey {
    fn public_key_der(&self) -> &[u8] {
        &self.public
    }

    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, KeyError> {
        let (reply, answered) = sync_channel(1);
        self.jobs
            .lock()
            .map_err(|_| KeyError::Sign("the TPM's queue was poisoned".to_owned()))?
            .send(SignJob {
                digest: Sha256::digest(message).into(),
                reply,
            })
            .map_err(|_| KeyError::Sign("the TPM's thread has ended".to_owned()))?;
        answered
            .recv()
            .map_err(|_| KeyError::Sign("the TPM's thread ended before it signed".to_owned()))?
    }

    fn custody(&self) -> KeyCustody {
        KeyCustody::Tpm
    }
}

impl std::fmt::Debug for TpmDeviceKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TpmDeviceKey").finish_non_exhaustive()
    }
}

struct Loaded {
    context: Context,
    key: KeyHandle,
    public: Vec<u8>,
    created: Option<String>,
}

impl Loaded {
    fn open(tcti: TctiNameConf, stored: Option<(Public, Private)>) -> Result<Self, KeyError> {
        let mut context = Context::new(tcti).map_err(|error| custody("the TPM", &error))?;
        let parent = context
            .execute_with_nullauth_session(|tpm| {
                tpm.create_primary(
                    Hierarchy::Owner,
                    storage_template()?,
                    None,
                    None,
                    None,
                    None,
                )
            })
            .map_err(|error| custody("the storage key", &error))?
            .key_handle;
        let (public, private, created) = match stored {
            Some((public, private)) => (public, private, None),
            None => {
                let made = context
                    .execute_with_nullauth_session(|tpm| {
                        let template = create_unrestricted_signing_ecc_public(
                            EccScheme::EcDsa(HashScheme::new(HashingAlgorithm::Sha256)),
                            EccCurve::NistP256,
                        )?;
                        tpm.create(parent, template, None, None, None, None)
                    })
                    .map_err(|error| custody("a new key", &error))?;
                let blob = encode(&made.out_public, &made.out_private)?;
                (made.out_public, made.out_private, Some(blob))
            }
        };
        let key = context
            .execute_with_nullauth_session(|tpm| tpm.load(parent, private, public.clone()))
            .map_err(|error| custody("the stored key — not this TPM's", &error))?;
        if let Err(error) = context.flush_context(parent.into()) {
            warn!(%error, "the TPM kept the storage key loaded; it frees it when this session ends");
        }
        Ok(Self {
            public: public_key_der(&public)?,
            context,
            key,
            created,
        })
    }

    fn serve(mut self, jobs: &Receiver<SignJob>) {
        for job in jobs {
            let signed = self.sign(&job.digest);
            if let Err(unsent) = job.reply.send(signed) {
                log_unheard(unsent.0);
            }
        }
    }

    fn sign(&mut self, digest: &[u8; 32]) -> Result<Vec<u8>, KeyError> {
        let digest = Digest::try_from(digest.to_vec()).map_err(|error| sign_failed(&error))?;
        // A null ticket: an unrestricted signing key signs any digest, and this one is ours.
        let ticket = HashcheckTicket::try_from(TPMT_TK_HASHCHECK {
            tag: TPM2_ST_HASHCHECK,
            hierarchy: TPM2_RH_NULL,
            digest: Default::default(),
        })
        .map_err(|error| sign_failed(&error))?;
        let key = self.key;
        let signature = self
            .context
            .execute_with_nullauth_session(|tpm| {
                tpm.sign(
                    key,
                    digest,
                    SignatureScheme::EcDsa {
                        hash_scheme: HashScheme::new(HashingAlgorithm::Sha256),
                    },
                    ticket,
                )
            })
            .map_err(|error| sign_failed(&error))?;
        let Signature::EcDsa(ecdsa) = signature else {
            return Err(KeyError::Sign(
                "the TPM answered with another scheme".to_owned(),
            ));
        };
        let signature = p256::ecdsa::Signature::from_scalars(
            field_bytes(ecdsa.signature_r().value())?,
            field_bytes(ecdsa.signature_s().value())?,
        )
        .map_err(|error| KeyError::Sign(format!("the TPM's signature is not P-256: {error}")))?;
        Ok(signature.to_der().as_bytes().to_vec())
    }
}

/// Logs a signing answer whose asker had already gone, so a refusal never vanishes.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn log_unheard(answer: Result<Vec<u8>, KeyError>) {
    match answer {
        Ok(_) => debug!("the TPM signed for an asker that had already given up"),
        Err(failure) => {
            warn!(%failure, "the TPM refused to sign, and the asker had already given up");
        }
    }
}

/// The storage key every device key is wrapped under: ECC P-256, restricted, derived from the
/// owner hierarchy's seed so the same template gives the same key.
fn storage_template() -> tss_esapi::Result<Public> {
    let attributes = ObjectAttributesBuilder::new()
        .with_fixed_tpm(true)
        .with_fixed_parent(true)
        .with_sensitive_data_origin(true)
        .with_user_with_auth(true)
        .with_restricted(true)
        .with_decrypt(true)
        .build()?;
    PublicBuilder::new()
        .with_public_algorithm(PublicAlgorithm::Ecc)
        .with_name_hashing_algorithm(HashingAlgorithm::Sha256)
        .with_object_attributes(attributes)
        .with_ecc_parameters(
            PublicEccParametersBuilder::new_restricted_decryption_key(
                SymmetricDefinitionObject::AES_128_CFB,
                EccCurve::NistP256,
            )
            .build()?,
        )
        .with_ecc_unique_identifier(EccPoint::default())
        .build()
}

fn public_key_der(public: &Public) -> Result<Vec<u8>, KeyError> {
    let Public::Ecc { unique, .. } = public else {
        return Err(KeyError::Malformed(
            "the TPM key is not an ECC key".to_owned(),
        ));
    };
    let point = EncodedPoint::from_affine_coordinates(
        &field_bytes(unique.x().value())?,
        &field_bytes(unique.y().value())?,
        false,
    );
    Option::<PublicKey>::from(PublicKey::from_encoded_point(&point))
        .ok_or_else(|| KeyError::Malformed("the TPM key's point is not on P-256".to_owned()))?
        .to_public_key_der()
        .map(|der| der.into_vec())
        .map_err(|error| KeyError::Malformed(error.to_string()))
}

/// A big-endian integer without leading zeros, as P-256's 32 bytes.
fn field_bytes(value: &[u8]) -> Result<FieldBytes, KeyError> {
    let significant = value
        .iter()
        .position(|byte| *byte != 0)
        .map_or(&[][..], |at| &value[at..]);
    if significant.len() > 32 {
        return Err(KeyError::Malformed(
            "a TPM integer is longer than P-256's".to_owned(),
        ));
    }
    let mut bytes = FieldBytes::default();
    bytes[32 - significant.len()..].copy_from_slice(significant);
    Ok(bytes)
}

fn encode(public: &Public, private: &Private) -> Result<String, KeyError> {
    let public = public
        .marshall()
        .map_err(|error| KeyError::Malformed(format!("the TPM key's public half: {error}")))?;
    Ok(format!(
        "{STORED}:{}:{}",
        URL_SAFE_NO_PAD.encode(public),
        URL_SAFE_NO_PAD.encode(private.value())
    ))
}

fn decode(kept: &str) -> Result<(Public, Private), KeyError> {
    let malformed = || KeyError::Malformed("the stored key is not a TPM key".to_owned());
    let rest = kept
        .strip_prefix(STORED)
        .and_then(|rest| rest.strip_prefix(':'))
        .ok_or_else(malformed)?;
    let (public, private) = rest.split_once(':').ok_or_else(malformed)?;
    let public = URL_SAFE_NO_PAD.decode(public).map_err(|_| malformed())?;
    let private = URL_SAFE_NO_PAD.decode(private).map_err(|_| malformed())?;
    Ok((
        Public::unmarshall(&public).map_err(|_| malformed())?,
        Private::try_from(private).map_err(|_| malformed())?,
    ))
}

fn custody(what: &str, error: &tss_esapi::Error) -> KeyError {
    KeyError::Custody(format!("{what}: {error}"))
}

fn sign_failed(error: &tss_esapi::Error) -> KeyError {
    KeyError::Sign(format!("the TPM did not sign: {error}"))
}
