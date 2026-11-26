use std::future::ready;

use crate::server::{
    Ed25519, KeyId, KeyProtection, KeyResidence, SignFuture, SignatureAlgorithm, Signer,
};

/// A server-side signing key for any signature algorithm, reached asynchronously.
///
/// For keys held by a network custody service; every [`Signer`] is also an
/// `AsyncSigner<Ed25519>`. Where a type implements both, call `AsyncSigner::sign(&signer, ..)`.
pub trait AsyncSigner<A: SignatureAlgorithm>: Send + Sync {
    /// The logical identifier of this signer's key, for logs and routing.
    fn key_id(&self) -> KeyId;

    /// The public key that verifies this signer's signatures.
    fn public_key(&self) -> A::PublicKey;

    /// How this signer's private key is protected.
    fn protection(&self) -> KeyProtection;

    /// Where this signer's private key is held.
    fn residence(&self) -> KeyResidence {
        if self.protection().is_accreditable() {
            KeyResidence::NonExportableCustody
        } else {
            KeyResidence::InProcess
        }
    }

    /// Whether producing a signature requires a second independent signer.
    fn dual_control(&self) -> bool {
        false
    }

    /// Produces a signature over `message`.
    ///
    /// # Errors
    /// The future resolves to [`KeyError::CustodyUnavailable`](crate::KeyError::CustodyUnavailable)
    /// (retryable) when the backend did not answer, or another [`KeyError`](crate::KeyError)
    /// when it refused; never an unverified signature.
    fn sign<'a>(&'a self, message: &'a [u8]) -> SignFuture<'a, A>;

    /// The algorithm this signer produces, derived from its type so it cannot disagree.
    fn algorithm(&self) -> crate::server::SignatureAlgorithmId {
        A::ID
    }
}

impl<S: Signer + ?Sized> AsyncSigner<Ed25519> for S {
    fn key_id(&self) -> KeyId {
        Signer::key_id(self)
    }

    fn public_key(&self) -> crate::VerifyingKey {
        Signer::public_key(self)
    }

    fn protection(&self) -> KeyProtection {
        Signer::protection(self)
    }

    fn dual_control(&self) -> bool {
        Signer::dual_control(self)
    }

    fn sign<'a>(&'a self, message: &'a [u8]) -> SignFuture<'a, Ed25519> {
        Box::pin(ready(Signer::sign(self, message)))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::server::providers::SoftwareSigner;
    use crate::server::{Es256, KeyResidence, SignatureAlgorithmId};

    #[tokio::test]
    async fn a_sync_signer_signs_through_the_async_seam() {
        let signer: Arc<dyn Signer> =
            Arc::new(SoftwareSigner::generate("issuer").expect("generate"));
        let message = b"one statement";
        let signature = AsyncSigner::<Ed25519>::sign(signer.as_ref(), message)
            .await
            .expect("signs");
        Ed25519::verify(
            &AsyncSigner::<Ed25519>::public_key(signer.as_ref()),
            message,
            &signature,
        )
        .expect("verifies");
    }

    #[test]
    fn an_in_process_software_key_resides_in_the_process() {
        let signer = SoftwareSigner::generate("issuer").expect("generate");
        assert_eq!(
            AsyncSigner::<Ed25519>::residence(&signer),
            KeyResidence::InProcess
        );
        let p256 =
            crate::server::providers::SoftwareP256Signer::generate("issuer").expect("generate");
        assert_eq!(
            AsyncSigner::<Es256>::residence(&p256),
            KeyResidence::InProcess
        );
    }

    #[test]
    fn a_hardware_posture_resides_in_non_exportable_custody() {
        struct Module(SoftwareSigner);
        impl Signer for Module {
            fn key_id(&self) -> KeyId {
                Signer::key_id(&self.0)
            }
            fn public_key(&self) -> crate::VerifyingKey {
                Signer::public_key(&self.0)
            }
            fn protection(&self) -> KeyProtection {
                KeyProtection::Hsm
            }
            fn sign(&self, message: &[u8]) -> Result<crate::Signature, crate::KeyError> {
                Signer::sign(&self.0, message)
            }
        }
        let module = Module(SoftwareSigner::generate("hsm").expect("generate"));
        assert_eq!(
            AsyncSigner::<Ed25519>::residence(&module),
            KeyResidence::NonExportableCustody
        );
    }

    #[test]
    fn the_algorithm_comes_from_the_type() {
        let signer = SoftwareSigner::generate("issuer").expect("generate");
        assert_eq!(
            AsyncSigner::<Ed25519>::algorithm(&signer),
            SignatureAlgorithmId::Ed25519
        );
        assert_eq!(Es256::ID, SignatureAlgorithmId::EcdsaP256Sha256);
    }

    #[test]
    fn signer_is_object_safe_for_both_algorithms() {
        fn holds(_: Option<Arc<dyn AsyncSigner<Ed25519>>>, _: Option<Arc<dyn AsyncSigner<Es256>>>) {
        }
        holds(None, None);
    }
}
