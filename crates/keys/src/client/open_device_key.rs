use tracing::info;

use super::{DeviceKey, SealedDeviceKey};
use crate::{KeyError, SecretStore};

#[cfg(all(feature = "client-tpm", target_os = "linux"))]
const LOCAL_TPM: &str = "/dev/tpmrm0";

/// The key `store` keeps under `account`, in the custody it was made in — or a new one, made
/// in the machine's TPM where it has one (Linux) and otherwise sealed in `store`.
///
/// A key keeps the custody it was enrolled with, since another would be a different device. A
/// TPM that is present but cannot be opened is refused, never replaced by a sealed key.
///
/// # Errors
/// [`KeyError::Custody`] when the key is a TPM's and the TPM cannot be reached or refuses;
/// [`KeyError::TpmUnreachable`] when a TPM is present and this account cannot open it; what
/// [`SealedDeviceKey::open_or_create`] refuses otherwise.
pub fn open_device_key(
    store: &dyn SecretStore,
    account: &str,
) -> Result<Box<dyn DeviceKey>, KeyError> {
    let in_tpm = format!("{account}.tpm");
    if store.load(&in_tpm)?.is_some() {
        return open_in_tpm(store, &in_tpm);
    }
    if store.load(account)?.is_none() && tpm_is_here()? {
        info!("this machine has a TPM; the device key is made in it");
        return open_in_tpm(store, &in_tpm);
    }
    SealedDeviceKey::open_or_create(store, account).map(|key| Box::new(key) as Box<dyn DeviceKey>)
}

#[cfg(all(feature = "client-tpm", target_os = "linux"))]
fn tpm_is_here() -> Result<bool, KeyError> {
    tpm_at(LOCAL_TPM)
}

#[cfg(not(all(feature = "client-tpm", target_os = "linux")))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "one signature with the TPM build, where the probe can fail"
)]
const fn tpm_is_here() -> Result<bool, KeyError> {
    Ok(false)
}

/// Whether the TPM at `path` can be used: absent is `false`, present and openable is `true`,
/// and present but refused is [`KeyError::TpmUnreachable`].
#[cfg(any(test, all(feature = "client-tpm", target_os = "linux")))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn tpm_at(path: &'static str) -> Result<bool, KeyError> {
    use std::io::ErrorKind;

    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
    {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            info!(
                path,
                "no TPM on this machine; the device key is sealed in the key store"
            );
            Ok(false)
        }
        Err(error) => {
            tracing::warn!(path, %error, "a TPM is present and this account cannot open it");
            Err(KeyError::TpmUnreachable {
                path,
                reason: error.to_string(),
            })
        }
    }
}

#[cfg(all(feature = "client-tpm", target_os = "linux"))]
fn open_in_tpm(store: &dyn SecretStore, account: &str) -> Result<Box<dyn DeviceKey>, KeyError> {
    super::TpmDeviceKey::open_or_create(&format!("device:{LOCAL_TPM}"), store, account)
        .map(|key| Box::new(key) as Box<dyn DeviceKey>)
}

#[cfg(not(all(feature = "client-tpm", target_os = "linux")))]
fn open_in_tpm(_store: &dyn SecretStore, _account: &str) -> Result<Box<dyn DeviceKey>, KeyError> {
    Err(KeyError::Custody(
        "this device's key is held by a TPM, and this build cannot reach one".to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{open_device_key, tpm_at};
    use crate::MemorySecretStore;
    use crate::client::KeyCustody;

    #[test]
    fn a_machine_without_a_tpm_seals_the_key_and_keeps_it() {
        let store = MemorySecretStore::new();
        let first = open_device_key(&store, "device").expect("made");
        let again = open_device_key(&store, "device").expect("opened");
        assert_eq!(first.public_key_der(), again.public_key_der());
        assert_eq!(again.custody(), KeyCustody::SealedInKeyStore);
    }

    /// A TPM key is never replaced by a sealed one.
    #[cfg(not(all(feature = "client-tpm", target_os = "linux")))]
    #[test]
    fn a_tpm_key_this_build_cannot_reach_is_refused_not_replaced() {
        use crate::SecretStore as _;
        use fundamental_core::secrets::SecretString;

        let store = MemorySecretStore::new();
        store
            .store(
                "device.tpm",
                &SecretString::new("tpm2-p256:v1:a:b".to_owned()),
            )
            .expect("kept");
        let refused = open_device_key(&store, "device").expect_err("refused");
        assert!(
            matches!(refused, crate::KeyError::Custody(_)),
            "{refused:?}"
        );
        assert!(
            store.load("device").expect("read").is_none(),
            "no sealed key was made"
        );
    }

    #[test]
    fn no_tpm_device_means_no_tpm() {
        assert!(matches!(tpm_at("/nonexistent/acme-tpm"), Ok(false)));
    }

    #[cfg(unix)]
    #[test]
    fn a_tpm_this_account_cannot_open_is_refused_with_the_reason() {
        use std::os::unix::fs::PermissionsExt as _;

        let path = std::env::temp_dir().join(format!("acme-tpm-{}", std::process::id()));
        std::fs::write(&path, b"").expect("written");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("closed");
        let leaked: &'static str = Box::leak(path.display().to_string().into_boxed_str());
        let refused = tpm_at(leaked);
        std::fs::remove_file(&path).expect("removed");
        if running_as_root() {
            return;
        }
        assert!(
            matches!(&refused, Err(crate::KeyError::TpmUnreachable { reason, .. }) if reason.contains("ermission")),
            "{refused:?}"
        );
    }

    #[cfg(unix)]
    fn running_as_root() -> bool {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status
                    .lines()
                    .find(|line| line.starts_with("Uid:"))
                    .map(|line| line.split_whitespace().nth(1) == Some("0"))
            })
            .unwrap_or(false)
    }
}
