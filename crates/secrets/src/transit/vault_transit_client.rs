//! [`VaultTransitClient`] — a [`KmsSignClient`] over a Vault or OpenBao Transit key.

use std::marker::PhantomData;
use std::sync::Arc;

use fundamental_core::Error as _;
use fundamental_keys::{
    DescribeFuture, Exportability, KeyError, KeyProtection, KmsKeyDescription, KmsSignClient,
    SignFuture,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::timeout;
use tracing::{debug, warn};

use crate::client::VaultClient;
use crate::error::SecretError;
use crate::transit::{TransitAlgorithm, TransitKeyInfo, TransitKeyRef, TransitSignerLimits};

/// A [`KmsSignClient`] for one pinned version of one Transit key.
///
/// Wrap it in [`KmsSigner`](fundamental_keys::KmsSigner) to sign. Protection is reported as
/// [`KeyProtection::Software`]: nothing in the engine's API attests hardware custody.
pub struct VaultTransitClient<A: TransitAlgorithm> {
    vault: VaultClient,
    key: TransitKeyRef,
    limits: TransitSignerLimits,
    in_flight: Arc<Semaphore>,
    algorithm: PhantomData<A>,
}

impl<A: TransitAlgorithm> VaultTransitClient<A> {
    /// A client for `key` through `vault`, bounded by `limits`.
    #[must_use]
    pub fn new(vault: VaultClient, key: TransitKeyRef, limits: TransitSignerLimits) -> Self {
        Self {
            vault,
            key,
            in_flight: Arc::new(Semaphore::new(limits.max_in_flight)),
            limits,
            algorithm: PhantomData,
        }
    }

    fn permit(&self) -> Result<OwnedSemaphorePermit, KeyError> {
        Arc::clone(&self.in_flight)
            .try_acquire_owned()
            .map_err(|_| {
                warn!(
                    vault.mount = %self.key.mount,
                    max_in_flight = self.limits.max_in_flight,
                    "transit signer saturated; refusing rather than queueing"
                );
                KeyError::CustodyUnavailable
            })
    }

    fn description(&self, info: TransitKeyInfo) -> Result<KmsKeyDescription<A>, KeyError> {
        if info.key_type != A::KEY_TYPE.as_str() {
            warn!(
                vault.mount = %self.key.mount,
                expected = %A::KEY_TYPE,
                found = %info.key_type,
                "transit key is of a different type than the signer expects"
            );
            return Err(KeyError::AlgorithmMismatch);
        }
        let encoded = info
            .public_keys
            .get(&self.key.version.get())
            .ok_or_else(|| KeyError::Custody("the pinned key version does not exist".to_owned()))?;
        let exportability = if info.exportable || info.allow_plaintext_backup {
            Exportability::Exportable
        } else {
            Exportability::NonExportable
        };
        Ok(KmsKeyDescription {
            public_key: A::parse_public_key(encoded)?,
            protection: KeyProtection::Software,
            exportability,
        })
    }

    fn signature(&self, returned: &str) -> Result<A::Signature, KeyError> {
        let expected_prefix = format!("vault:v{}:", self.key.version);
        let encoded = returned.strip_prefix(&expected_prefix).ok_or_else(|| {
            warn!(
                vault.mount = %self.key.mount,
                pinned_version = self.key.version.get(),
                "transit returned a signature from a version other than the pinned one"
            );
            KeyError::Custody("signature from an unexpected key version".to_owned())
        })?;
        A::parse_signature(encoded)
    }
}

impl<A: TransitAlgorithm> KmsSignClient<A> for VaultTransitClient<A> {
    fn describe(&self) -> DescribeFuture<'_, A> {
        Box::pin(async move {
            let _permit = self.permit()?;
            let info = timeout(
                self.limits.deadline,
                self.vault
                    .transit_read_signing_key(&self.key.mount, &self.key.name),
            )
            .await
            .map_err(|_| deadline_passed(&self.key))?
            .map_err(|error| custody_error(&error, &self.key))?;
            self.description(info)
        })
    }

    fn sign<'a>(&'a self, message: &'a [u8]) -> SignFuture<'a, A> {
        Box::pin(async move {
            let _permit = self.permit()?;
            let mut extra = serde_json::Map::new();
            A::sign_body(&mut extra);
            let returned = timeout(
                self.limits.deadline,
                self.vault
                    .transit_sign(&self.key, A::sign_path_suffix(), extra, message),
            )
            .await
            .map_err(|_| deadline_passed(&self.key))?
            .map_err(|error| custody_error(&error, &self.key))?;
            debug!(vault.mount = %self.key.mount, key_version = self.key.version.get(), "transit signature received");
            self.signature(&returned)
        })
    }
}

fn deadline_passed(key: &TransitKeyRef) -> KeyError {
    warn!(vault.mount = %key.mount, "transit call exceeded its deadline");
    KeyError::CustodyUnavailable
}

/// Maps an engine failure to the signing seam's errors, without the key's name or the engine's wording.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn custody_error(error: &SecretError, key: &TransitKeyRef) -> KeyError {
    match error {
        SecretError::Transport(_) => {
            warn!(vault.mount = %key.mount, error = %error, "transit unreachable");
            KeyError::CustodyUnavailable
        }
        SecretError::Api { status, .. } if error.is_retryable() => {
            warn!(vault.mount = %key.mount, http.status = status, "transit unavailable");
            KeyError::CustodyUnavailable
        }
        SecretError::Api { status, .. } => {
            warn!(vault.mount = %key.mount, http.status = status, "transit refused the request");
            KeyError::Custody(format!(
                "the custody service refused the request ({status})"
            ))
        }
        other => {
            warn!(vault.mount = %key.mount, error = %other, "transit call failed");
            KeyError::Custody("the custody service returned an unusable answer".to_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use fundamental_config::settings::{VaultAuth, VaultSettings};
    use fundamental_core::secrets::InMemorySecret;
    use fundamental_keys::Es256;

    use super::*;
    use crate::transit::{TransitKeyName, TransitMount};

    async fn unreachable_vault() -> VaultClient {
        let settings = VaultSettings {
            enabled: true,
            // Port 9 on loopback: nothing listens, so the connect is refused at once.
            address: "http://127.0.0.1:9".to_owned(),
            auth: VaultAuth::Token {
                token: InMemorySecret::new("unused".to_owned()),
            },
            ..VaultSettings::default()
        };
        VaultClient::from_settings(&settings)
            .await
            .expect("token auth needs no round trip")
            .expect("enabled")
    }

    fn key() -> TransitKeyRef {
        TransitKeyRef::new(
            TransitMount::parse("transit").expect("valid"),
            TransitKeyName::parse("t-test").expect("valid"),
            NonZeroU32::MIN,
        )
    }

    #[tokio::test]
    async fn an_unreachable_custody_backend_is_retryable() {
        use fundamental_core::Error as _;
        let client = VaultTransitClient::<Es256>::new(
            unreachable_vault().await,
            key(),
            TransitSignerLimits::default(),
        );
        let error = client.sign(b"statement").await.expect_err("unreachable");
        assert!(matches!(error, KeyError::CustodyUnavailable));
        assert_eq!(error.error_code(), "KEYS_008");
        assert!(error.is_retryable());
    }

    #[tokio::test]
    async fn a_saturated_signer_refuses_rather_than_queues() {
        let limits = TransitSignerLimits {
            max_in_flight: 1,
            ..TransitSignerLimits::default()
        };
        let client = VaultTransitClient::<Es256>::new(unreachable_vault().await, key(), limits);
        let _held = client.permit().expect("the one permit");
        let error = client.sign(b"statement").await.expect_err("saturated");
        assert!(matches!(error, KeyError::CustodyUnavailable));
    }

    #[test]
    fn a_signature_from_another_version_is_refused() {
        let vault_future = unreachable_vault();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let client = VaultTransitClient::<Es256>::new(
            runtime.block_on(vault_future),
            key(),
            TransitSignerLimits::default(),
        );
        assert!(matches!(
            client.signature("vault:v2:xAZq-wszmSMQui_1SEIi65nAjdqyyi5Tr76OQmDppD5Tl3INE4bXQ0h3evTQ6TD1gGJyUBJJvIZeUwstuB8D6g"),
            Err(KeyError::Custody(_))
        ));
        assert!(client
            .signature("vault:v1:xAZq-wszmSMQui_1SEIi65nAjdqyyi5Tr76OQmDppD5Tl3INE4bXQ0h3evTQ6TD1gGJyUBJJvIZeUwstuB8D6g")
            .is_ok());
    }

    #[test]
    fn a_key_of_another_type_is_refused() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let client = VaultTransitClient::<Es256>::new(
            runtime.block_on(unreachable_vault()),
            key(),
            TransitSignerLimits::default(),
        );
        let info = TransitKeyInfo {
            key_type: "ed25519".to_owned(),
            exportable: false,
            allow_plaintext_backup: false,
            latest_version: 1,
            public_keys: [(1, "mw0hx09eOCQl5Pb6dK4fj2UeqyIsUj66XUhreBeTAPA=".to_owned())].into(),
        };
        assert!(matches!(
            client.description(info),
            Err(KeyError::AlgorithmMismatch)
        ));
    }
}
