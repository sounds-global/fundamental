use std::marker::PhantomData;

use tracing::{error, instrument};

use crate::KeyError;
use crate::server::providers::{Exportability, KmsKeyDescription, KmsSignClient};
use crate::server::{
    AsyncSigner, KeyId, KeyProtection, KeyResidence, SignFuture, SignatureAlgorithm,
};

/// An [`AsyncSigner`] whose key is held by a network custody service.
///
/// An exportable key never opens; the public key is pinned when it opens; every signature is
/// verified against it before it is returned.
pub struct KmsSigner<C, A>
where
    C: KmsSignClient<A>,
    A: SignatureAlgorithm,
{
    key_id: KeyId,
    public_key: A::PublicKey,
    protection: KeyProtection,
    client: C,
    algorithm: PhantomData<A>,
}

impl<C, A> KmsSigner<C, A>
where
    C: KmsSignClient<A>,
    A: SignatureAlgorithm,
{
    /// Opens a signer, reading the key's description from the service.
    ///
    /// # Errors
    /// [`KeyError::ExportableKeyRefused`] if the service reports an exportable key, or the
    /// client's error if the key cannot be described.
    #[instrument(name = "keys.kms_open", skip_all, fields(key_id = tracing::field::Empty, algorithm = %A::ID), err(Debug))]
    pub async fn open(key_id: impl Into<KeyId>, client: C) -> Result<Self, KeyError> {
        let key_id = key_id.into();
        tracing::Span::current().record("key_id", tracing::field::display(&key_id));
        let description = client.describe().await?;
        Self::from_description(key_id, client, description)
    }

    /// Builds a signer from a description recorded at enrolment, without asking the service.
    ///
    /// # Errors
    /// The same refusals as [`open`](Self::open).
    pub fn from_description(
        key_id: impl Into<KeyId>,
        client: C,
        description: KmsKeyDescription<A>,
    ) -> Result<Self, KeyError> {
        if description.exportability == Exportability::Exportable {
            return Err(KeyError::ExportableKeyRefused);
        }
        Ok(Self {
            key_id: key_id.into(),
            public_key: description.public_key,
            protection: description.protection,
            client,
            algorithm: PhantomData,
        })
    }
}

impl<C, A> AsyncSigner<A> for KmsSigner<C, A>
where
    C: KmsSignClient<A>,
    A: SignatureAlgorithm,
{
    fn key_id(&self) -> KeyId {
        self.key_id.clone()
    }

    fn public_key(&self) -> A::PublicKey {
        self.public_key.clone()
    }

    fn protection(&self) -> KeyProtection {
        self.protection
    }

    /// Always non-exportable custody: an exportable key never opened.
    fn residence(&self) -> KeyResidence {
        KeyResidence::NonExportableCustody
    }

    fn sign<'a>(&'a self, message: &'a [u8]) -> SignFuture<'a, A> {
        Box::pin(async move {
            let signature = self.client.sign(message).await?;
            if A::verify(&self.public_key, message, &signature).is_err() {
                error!(
                    event = "keys.signature_self_check_failed",
                    key_id = %self.key_id,
                    algorithm = %A::ID,
                    "a custody backend returned a signature the pinned public key does not verify"
                );
                return Err(KeyError::SignatureSelfCheckFailed);
            }
            Ok(signature)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use crate::server::providers::DescribeFuture;
    use crate::server::providers::SoftwareP256Signer;
    use crate::server::{Es256, KeyResidence, P256Signature};

    struct FakeService {
        key: SoftwareP256Signer,
        protection: KeyProtection,
        exportability: Exportability,
        corrupt: AtomicBool,
    }

    impl FakeService {
        fn new(protection: KeyProtection, exportability: Exportability) -> Self {
            Self {
                key: SoftwareP256Signer::generate("svc").expect("generate"),
                protection,
                exportability,
                corrupt: AtomicBool::new(false),
            }
        }
    }

    impl KmsSignClient<Es256> for FakeService {
        fn describe(&self) -> DescribeFuture<'_, Es256> {
            let description = KmsKeyDescription {
                public_key: AsyncSigner::public_key(&self.key),
                protection: self.protection,
                exportability: self.exportability,
            };
            Box::pin(async move { Ok(description) })
        }

        #[expect(clippy::excessive_nesting, reason = "readability baseline")]
        fn sign<'a>(&'a self, message: &'a [u8]) -> SignFuture<'a, Es256> {
            Box::pin(async move {
                let signature = AsyncSigner::sign(&self.key, message).await?;
                if self.corrupt.load(Ordering::SeqCst) {
                    let other = SoftwareP256Signer::generate("other")?;
                    return AsyncSigner::sign(&other, message).await;
                }
                Ok::<P256Signature, KeyError>(signature)
            })
        }
    }

    #[tokio::test]
    async fn an_es256_signature_verifies_under_its_public_key() {
        let signer = KmsSigner::open(
            "tenant-k1",
            FakeService::new(KeyProtection::Hsm, Exportability::NonExportable),
        )
        .await
        .expect("opens");
        let message = b"entity configuration";
        let signature = signer.sign(message).await.expect("signs");
        Es256::verify(&signer.public_key(), message, &signature).expect("verifies");
    }

    #[tokio::test]
    async fn kms_signer_reports_the_backends_own_protection() {
        let signer = KmsSigner::open(
            "tenant-k1",
            FakeService::new(KeyProtection::Software, Exportability::NonExportable),
        )
        .await
        .expect("a software custody service opens, and says so");
        assert_eq!(signer.protection(), KeyProtection::Software);
        assert!(!signer.protection().is_accreditable());
    }

    /// A custody-held software key is told apart from an in-process key.
    #[tokio::test]
    async fn a_service_held_software_key_resides_in_non_exportable_custody() {
        let signer = KmsSigner::open(
            "tenant-k1",
            FakeService::new(KeyProtection::Software, Exportability::NonExportable),
        )
        .await
        .expect("opens");
        assert_eq!(signer.residence(), KeyResidence::NonExportableCustody);
        assert_eq!(signer.protection(), KeyProtection::Software);
    }

    #[tokio::test]
    async fn kms_signer_refuses_an_exportable_key() {
        let opened = KmsSigner::open(
            "tenant-k1",
            FakeService::new(KeyProtection::Hsm, Exportability::Exportable),
        )
        .await;
        let error = opened.err().expect("refused");
        assert!(matches!(error, KeyError::ExportableKeyRefused));
        assert_eq!(fundamental_core::Error::error_code(&error), "KEYS_010");
    }

    #[tokio::test]
    async fn kms_signer_withholds_a_signature_the_pinned_key_rejects() {
        let service = FakeService::new(KeyProtection::Hsm, Exportability::NonExportable);
        service.corrupt.store(true, Ordering::SeqCst);
        let signer = KmsSigner::open("tenant-k1", service).await.expect("opens");
        let error = signer.sign(b"statement").await.expect_err("withheld");
        assert!(matches!(error, KeyError::SignatureSelfCheckFailed));
    }
}
