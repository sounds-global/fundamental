use std::fmt;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use fundamental_config::settings::{NkeyPublicKey, NkeySigner};
use fundamental_secrets::VaultClient;
use fundamental_secrets::transit::{
    TransitKeyInfo, TransitKeyName, TransitKeyRef, TransitMount, ed25519_public_key_bytes,
    ed25519_signature_bytes,
};
use tokio::time::{Instant, timeout};
use tracing::{debug, info, instrument};

use super::nkey_user_public_key;
use crate::StoreAnswer;
use crate::error::{NatsError, NatsResult};

/// The longest nonce signed, so a hostile server cannot have arbitrary data signed.
const NONCE_MAX: usize = 64;

/// How long one Transit call may take, under the five-second connect timeout.
const DEADLINE: Duration = Duration::from_secs(3);

/// Signs the server's nonce for a NATS NKey whose seed is a non-exportable Ed25519 Transit key.
///
/// Each signature is checked against the configured public key before it is sent. The key
/// version is pinned at [`connect`](Self::connect), so an engine rotation cannot change the
/// signer. Cheap to clone.
#[derive(Clone)]
pub struct NkeyTransitSigner {
    inner: Arc<Inner>,
}

struct Inner {
    vault: VaultClient,
    key: TransitKeyRef,
    public_key: NkeyPublicKey,
    verifier: nkeys::KeyPair,
    signer: String,
}

impl fmt::Debug for NkeyTransitSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NkeyTransitSigner")
            .field("signer", &self.inner.signer)
            .field("version", &self.inner.key.version)
            .field("public_key", &self.inner.public_key)
            .finish()
    }
}

impl NkeyTransitSigner {
    /// Finds the version of `signer`'s key whose public key is `public_key`, and signs with it.
    ///
    /// # Errors
    /// - [`NatsError::NkeyUnusable`] (`NATS_024`) when no secret store was given, a name is
    ///   malformed, `public_key` is not a user NKey, or the key is not a non-exportable `ed25519`
    ///   key with a version having `public_key`.
    /// - [`NatsError::NkeySignerFailed`] (`NATS_025`) when the key cannot be read.
    #[instrument(skip_all, fields(nkey.signer = %signer, nkey.public_key = %public_key), err(Debug))]
    pub async fn connect(
        vault: Option<&VaultClient>,
        signer: &NkeySigner,
        public_key: &NkeyPublicKey,
    ) -> NatsResult<Self> {
        let label = signer.to_string();
        let unusable = |reason: String| NatsError::NkeyUnusable {
            signer: label.clone(),
            public_key: public_key.to_string(),
            reason,
        };
        let Some(vault) = vault else {
            return Err(unusable(
                "no secret store was given; pass the client built from `[vault]` to \
                 `NatsConfig::from_settings`"
                    .to_owned(),
            ));
        };
        let NkeySigner::VaultTransit { mount, key } = signer;
        let mount = TransitMount::parse(mount).map_err(|error| unusable(error.to_string()))?;
        let name = TransitKeyName::parse(key).map_err(|error| unusable(error.to_string()))?;
        let verifier = nkeys::KeyPair::from_public_key(public_key.as_str())
            .map_err(|error| unusable(format!("not a valid NKey: {error}")))?;
        let (_, wanted) = nkeys::from_public_key(public_key.as_str())
            .map_err(|error| unusable(format!("not a valid NKey: {error}")))?;

        let info = match timeout(DEADLINE, vault.transit_read_signing_key(&mount, &name)).await {
            Ok(Ok(info)) => info,
            Ok(Err(failure)) => {
                return Err(NatsError::NkeySignerFailed {
                    signer: label,
                    answer: StoreAnswer::of(&failure),
                    reason: format!("its key could not be read: {failure}"),
                });
            }
            Err(_) => {
                return Err(NatsError::NkeySignerFailed {
                    signer: label,
                    answer: StoreAnswer::Unavailable,
                    reason: format!("its key was not read within {DEADLINE:?}"),
                });
            }
        };
        let version = usable_version(&info, &wanted).map_err(unusable)?;
        info!(
            nkey.signer = %label,
            nkey.public_key = %public_key,
            key_version = version.get(),
            "NATS NKey signs with a Transit key; the seed stays in the secret store"
        );
        Ok(Self {
            inner: Arc::new(Inner {
                vault: vault.clone(),
                key: TransitKeyRef::new(mount, name, version),
                public_key: public_key.clone(),
                verifier,
                signer: label,
            }),
        })
    }

    /// The public key this signer signs as.
    #[must_use]
    pub fn public_key(&self) -> &NkeyPublicKey {
        &self.inner.public_key
    }

    /// Which signer this is: `vault_transit <mount>/<key>`.
    #[must_use]
    pub fn signer(&self) -> &str {
        &self.inner.signer
    }

    /// Signs `nonce`, and checks the signature against the public key before returning it.
    ///
    /// # Errors
    /// - [`NatsError::NkeySignerFailed`] (`NATS_025`) when `nonce` is not up to 64 base64url
    ///   characters, or the engine fails, refuses, misses the deadline or answers with no
    ///   signature.
    /// - [`NatsError::NkeyUnusable`] (`NATS_024`) when the signature does not verify.
    #[instrument(skip_all, fields(nkey.signer = %self.inner.signer, key_version = self.inner.key.version.get()), err(Debug))]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    pub async fn sign(&self, nonce: &[u8]) -> NatsResult<[u8; 64]> {
        let inner = &self.inner;
        let started = Instant::now();
        let failed = |answer, reason: String| NatsError::NkeySignerFailed {
            signer: inner.signer.clone(),
            answer,
            reason: format!("{reason} (after {} ms)", started.elapsed().as_millis()),
        };
        if nonce.is_empty()
            || nonce.len() > NONCE_MAX
            || !nonce
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
        {
            return Err(failed(
                StoreAnswer::Refused,
                "the server's nonce is not a NATS nonce; it is refused unsigned".to_owned(),
            ));
        }
        let returned = match timeout(
            DEADLINE,
            inner
                .vault
                .transit_sign(&inner.key, "", serde_json::Map::new(), nonce),
        )
        .await
        {
            Ok(Ok(returned)) => returned,
            Ok(Err(failure)) => {
                return Err(failed(
                    StoreAnswer::of(&failure),
                    format!("the nonce could not be signed: {failure}"),
                ));
            }
            Err(_) => {
                return Err(failed(
                    StoreAnswer::Unavailable,
                    format!("the nonce was not signed within {DEADLINE:?}"),
                ));
            }
        };
        let prefix = format!("vault:v{}:", inner.key.version);
        let Some(encoded) = returned.strip_prefix(&prefix) else {
            return Err(NatsError::NkeyUnusable {
                signer: inner.signer.clone(),
                public_key: inner.public_key.to_string(),
                reason: format!("the engine signed with a version other than {prefix}"),
            });
        };
        let signature = ed25519_signature_bytes(encoded).ok_or_else(|| {
            failed(
                StoreAnswer::Refused,
                "the engine answered with something that is not a 64-byte signature".to_owned(),
            )
        })?;
        inner
            .verifier
            .verify(nonce, &signature)
            .map_err(|error| NatsError::NkeyUnusable {
                signer: inner.signer.clone(),
                public_key: inner.public_key.to_string(),
                reason: format!("a signature did not verify against the public key: {error}"),
            })?;
        debug!(
            nkey.signer = %inner.signer,
            elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "nonce signed"
        );
        Ok(signature)
    }
}

fn usable_version(info: &TransitKeyInfo, wanted: &[u8; 32]) -> Result<NonZeroU32, String> {
    if info.key_type != "ed25519" {
        return Err(format!(
            "the key is {}; an NKey is an ed25519 key",
            info.key_type
        ));
    }
    if info.exportable || info.allow_plaintext_backup {
        return Err(
            "the key may leave the secret store (exportable or plaintext backup); an NKey's \
             seed must not"
                .to_owned(),
        );
    }
    let mut versions = Vec::with_capacity(info.public_keys.len());
    for (version, encoded) in &info.public_keys {
        let Some(raw) = ed25519_public_key_bytes(encoded) else {
            versions.push(format!("v{version} unreadable"));
            continue;
        };
        if &raw == wanted {
            return NonZeroU32::new(*version)
                .ok_or_else(|| "the engine reported a version 0".to_owned());
        }
        versions.push(format!("v{version} {}", nkey_user_public_key(&raw)));
    }
    Err(format!(
        "no version of the key has this public key; its versions are: {}",
        versions.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::usable_version;
    use fundamental_secrets::transit::TransitKeyInfo;

    const RECORDED: &str = "mw0hx09eOCQl5Pb6dK4fj2UeqyIsUj66XUhreBeTAPA=";

    fn info(key_type: &str, exportable: bool) -> TransitKeyInfo {
        TransitKeyInfo {
            key_type: key_type.to_owned(),
            exportable,
            allow_plaintext_backup: false,
            latest_version: 2,
            public_keys: BTreeMap::from([(1, "AAAA".to_owned()), (2, RECORDED.to_owned())]),
        }
    }

    fn recorded() -> [u8; 32] {
        fundamental_secrets::transit::ed25519_public_key_bytes(RECORDED).expect("recorded key")
    }

    #[test]
    fn the_version_holding_the_public_key_is_pinned() {
        assert_eq!(
            usable_version(&info("ed25519", false), &recorded()).map(|v| v.get()),
            Ok(2)
        );
    }

    #[test]
    fn a_key_of_another_type_or_one_that_may_leave_the_store_is_refused() {
        let refused = usable_version(&info("ecdsa-p256", false), &recorded()).expect_err("type");
        assert!(refused.contains("an NKey is an ed25519 key"), "{refused}");
        let refused = usable_version(&info("ed25519", true), &recorded()).expect_err("export");
        assert!(refused.contains("may leave the secret store"), "{refused}");
    }

    #[test]
    fn a_public_key_no_version_holds_is_refused_naming_what_each_holds() {
        let refused = usable_version(&info("ed25519", false), &[7; 32]).expect_err("no match");
        assert!(refused.contains("v1 unreadable"), "{refused}");
        assert!(refused.contains("v2 U"), "{refused}");
    }
}
