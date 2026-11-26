use cryptoki::context::{CInitializeArgs, Pkcs11};
use cryptoki::mechanism::Mechanism;
use cryptoki::object::{Attribute, AttributeType, ObjectClass, ObjectHandle};
use cryptoki::session::{Session, UserType};
use cryptoki::slot::Slot;
use cryptoki::types::AuthPin;
use tracing::instrument;

use crate::server::providers::Pkcs11Config;
use crate::server::{KeyId, KeyProtection, Signer};
use crate::{KeyError, Signature, VerifyingKey};

/// A [`Signer`] backed by a **non-exportable** PKCS#11 HSM key.
///
/// Opens a fresh logged-in session per signature, so no non-`Sync` session is held.
pub struct Pkcs11Signer {
    pkcs11: Pkcs11,
    slot: Slot,
    key_label: String,
    pin: AuthPin,
    key_id: KeyId,
    public_key: VerifyingKey,
}

impl Pkcs11Signer {
    /// Opens a PKCS#11-backed signer, resolving the Ed25519 public key from the key pair.
    ///
    /// # Errors
    /// Returns [`KeyError::Custody`] if the module cannot be initialised, the token
    /// or key is not found, login fails, or the public key cannot be read.
    #[instrument(name = "keys.hsm_open", skip_all, fields(token = %config.token_label, key = %config.key_label))]
    pub fn open(config: Pkcs11Config) -> Result<Self, KeyError> {
        reported(Self::open_in_module(config), "the HSM signer did not open")
    }

    fn open_in_module(config: Pkcs11Config) -> Result<Self, KeyError> {
        let pkcs11 = initialised_module(&config)?;
        let slot = find_slot(&pkcs11, &config.token_label)?;
        let pin = login_pin(&config)?;
        let public_key = read_public_key(&pkcs11, slot, &pin, &config.key_label)?;
        Ok(Self {
            pkcs11,
            slot,
            key_label: config.key_label,
            pin,
            key_id: config.key_id,
            public_key,
        })
    }
}

impl Signer for Pkcs11Signer {
    fn key_id(&self) -> KeyId {
        self.key_id.clone()
    }

    fn public_key(&self) -> VerifyingKey {
        self.public_key
    }

    fn protection(&self) -> KeyProtection {
        KeyProtection::Hsm
    }

    #[instrument(name = "keys.hsm_sign", skip_all, fields(key_id = %self.key_id))]
    fn sign(&self, message: &[u8]) -> Result<Signature, KeyError> {
        reported(
            sign_in_module(&self.pkcs11, self.slot, &self.pin, &self.key_label, message),
            "the HSM did not sign",
        )
    }
}

/// Logs a failure inside the caller's span; `#[instrument(err)]` would trip cognitive complexity.
fn reported<T>(result: Result<T, KeyError>, what: &str) -> Result<T, KeyError> {
    result.inspect_err(|error| tracing::error!(?error, "{what}"))
}

fn initialised_module(config: &Pkcs11Config) -> Result<Pkcs11, KeyError> {
    let pkcs11 =
        Pkcs11::new(&config.module_path).map_err(|error| custody("load module", &error))?;
    pkcs11
        .initialize(CInitializeArgs::OsThreads)
        .map_err(|error| custody("initialize module", &error))?;
    Ok(pkcs11)
}

/// The login PIN; absent fails closed rather than attempting an unauthenticated login.
fn login_pin(config: &Pkcs11Config) -> Result<AuthPin, KeyError> {
    config
        .pin
        .expose()
        .map(|pin| AuthPin::new(pin.to_owned()))
        .ok_or_else(|| KeyError::Custody("PKCS#11 login PIN is absent".to_owned()))
}

fn read_public_key(
    pkcs11: &Pkcs11,
    slot: Slot,
    pin: &AuthPin,
    key_label: &str,
) -> Result<VerifyingKey, KeyError> {
    let session = login_session(pkcs11, slot, pin)?;
    let handle = find_object(&session, key_label, ObjectClass::PUBLIC_KEY)?;
    Ok(VerifyingKey::from_bytes(read_ed25519_point(
        &session, handle,
    )?))
}

/// Signs `message` with `CKM_EDDSA` inside the module, refusing anything but 64 bytes.
fn sign_in_module(
    pkcs11: &Pkcs11,
    slot: Slot,
    pin: &AuthPin,
    key_label: &str,
    message: &[u8],
) -> Result<Signature, KeyError> {
    let session = login_session(pkcs11, slot, pin)?;
    let handle = find_object(&session, key_label, ObjectClass::PRIVATE_KEY)?;
    let signature = session
        .sign(&Mechanism::Eddsa, handle, message)
        .map_err(|error| custody("hsm sign", &error))?;
    let bytes: [u8; 64] = signature
        .try_into()
        .map_err(|_| KeyError::Custody("HSM returned a non-64-byte signature".to_owned()))?;
    Ok(Signature::from_bytes(bytes))
}

fn find_slot(pkcs11: &Pkcs11, token_label: &str) -> Result<Slot, KeyError> {
    let slots = pkcs11
        .get_slots_with_token()
        .map_err(|error| custody("list slots", &error))?;
    for slot in slots {
        if let Ok(info) = pkcs11.get_token_info(slot)
            && info.label().trim() == token_label
        {
            return Ok(slot);
        }
    }
    Err(KeyError::Custody(format!(
        "no PKCS#11 token labeled {token_label}"
    )))
}

fn login_session(pkcs11: &Pkcs11, slot: Slot, pin: &AuthPin) -> Result<Session, KeyError> {
    let session = pkcs11
        .open_ro_session(slot)
        .map_err(|error| custody("open session", &error))?;
    session
        .login(UserType::User, Some(pin))
        .map_err(|error| custody("login", &error))?;
    Ok(session)
}

fn find_object(
    session: &Session,
    label: &str,
    class: ObjectClass,
) -> Result<ObjectHandle, KeyError> {
    let template = [
        Attribute::Label(label.as_bytes().to_vec()),
        Attribute::Class(class),
    ];
    let handles = session
        .find_objects(&template)
        .map_err(|error| custody("find object", &error))?;
    handles
        .into_iter()
        .next()
        .ok_or_else(|| KeyError::Custody(format!("no PKCS#11 object labeled {label}")))
}

/// Reads the raw Ed25519 public key from `CKA_EC_POINT`, DER-wrapped or raw.
fn read_ed25519_point(session: &Session, handle: ObjectHandle) -> Result<[u8; 32], KeyError> {
    let attrs = session
        .get_attributes(handle, &[AttributeType::EcPoint])
        .map_err(|error| custody("read public key", &error))?;
    let point = attrs
        .into_iter()
        .find_map(|attr| match attr {
            Attribute::EcPoint(bytes) => Some(bytes),
            _ => None,
        })
        .ok_or_else(|| KeyError::Custody("public key has no EC point".to_owned()))?;
    let raw: &[u8] = match point.as_slice() {
        [0x04, 0x20, rest @ ..] if rest.len() == 32 => rest,
        raw if raw.len() == 32 => raw,
        other => {
            return Err(KeyError::Custody(format!(
                "unexpected EC point length {}",
                other.len()
            )));
        }
    };
    raw.try_into()
        .map_err(|_| KeyError::Custody("EC point is not 32 bytes".to_owned()))
}

/// Maps a PKCS#11 error to [`KeyError::Custody`] naming the step, never the key or PIN.
fn custody(step: &str, error: &cryptoki::error::Error) -> KeyError {
    KeyError::Custody(format!("{step}: {error}"))
}
