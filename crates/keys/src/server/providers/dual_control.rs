use std::sync::Arc;

use tracing::instrument;

use crate::server::{KeyId, KeyProtection, Signer};
use crate::{KeyError, Signature, VerifyingKey};

/// A signer that produces a signature only under **dual control**: two independent
/// signers must both act.
///
/// A guardian signs the same message first and the operator signs only if that verifies;
/// callers see one signature, under the operator key.
pub struct DualControlSigner {
    operator: Arc<dyn Signer>,
    guardian: Arc<dyn Signer>,
    guardian_key: VerifyingKey,
}

impl DualControlSigner {
    /// Composes an `operator` and a `guardian` [`Signer`] into a dual-control signer.
    pub fn new(operator: Arc<dyn Signer>, guardian: Arc<dyn Signer>) -> Self {
        let guardian_key = guardian.public_key();
        Self {
            operator,
            guardian,
            guardian_key,
        }
    }
}

impl Signer for DualControlSigner {
    fn key_id(&self) -> KeyId {
        self.operator.key_id()
    }

    fn public_key(&self) -> VerifyingKey {
        self.operator.public_key()
    }

    fn protection(&self) -> KeyProtection {
        self.operator.protection()
    }

    fn dual_control(&self) -> bool {
        true
    }

    #[instrument(name = "keys.dual_control_sign", skip_all, fields(key_id = %self.operator.key_id()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn sign(&self, message: &[u8]) -> Result<Signature, KeyError> {
        let confirmation = self
            .guardian
            .sign(message)
            .map_err(|error| KeyError::DualControl(format!("guardian refused: {error}")))?;
        self.guardian_key
            .verify(message, &confirmation)
            .map_err(|_| {
                KeyError::DualControl("guardian confirmation did not verify".to_owned())
            })?;
        self.operator.sign(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::providers::SoftwareSigner;

    fn software(name: &str) -> Arc<dyn Signer> {
        Arc::new(SoftwareSigner::generate(name).expect("generate"))
    }

    struct RefusingGuardian(VerifyingKey);
    impl Signer for RefusingGuardian {
        fn key_id(&self) -> KeyId {
            KeyId::new("guardian")
        }
        fn public_key(&self) -> VerifyingKey {
            self.0
        }
        fn protection(&self) -> KeyProtection {
            KeyProtection::Software
        }
        fn sign(&self, _message: &[u8]) -> Result<Signature, KeyError> {
            Err(KeyError::Custody("guardian offline".to_owned()))
        }
    }

    const MESSAGE: &[u8] = b"cosignature/v1\ntime 7\nacme.audit.test\n";

    #[test]
    fn both_signers_present_produces_the_operators_signature() {
        let operator = software("witness.op");
        let operator_key = operator.public_key();
        let guardian = software("witness.guardian");
        let dual = DualControlSigner::new(operator, guardian);

        let signature = dual.sign(MESSAGE).expect("dual-control sign");
        operator_key
            .verify(MESSAGE, &signature)
            .expect("published signature is the operator's");
        assert!(dual.dual_control());
    }

    #[test]
    fn a_guardian_refusal_yields_no_signature() {
        let operator = software("witness.op");
        let guardian = Arc::new(RefusingGuardian(software("g").public_key()));
        let dual = DualControlSigner::new(operator, guardian);
        assert!(matches!(dual.sign(MESSAGE), Err(KeyError::DualControl(_))));
    }

    #[test]
    fn a_guardian_confirmation_over_a_different_message_is_refused() {
        struct WrongMessageGuardian(SoftwareSigner);
        impl Signer for WrongMessageGuardian {
            fn key_id(&self) -> KeyId {
                self.0.key_id()
            }
            fn public_key(&self) -> VerifyingKey {
                self.0.public_key()
            }
            fn protection(&self) -> KeyProtection {
                KeyProtection::Software
            }
            fn sign(&self, _message: &[u8]) -> Result<Signature, KeyError> {
                self.0.sign(b"a different message entirely")
            }
        }
        let operator = software("witness.op");
        let guardian = Arc::new(WrongMessageGuardian(
            SoftwareSigner::generate("witness.guardian").expect("generate"),
        ));
        let dual = DualControlSigner::new(operator, guardian);
        assert!(matches!(dual.sign(MESSAGE), Err(KeyError::DualControl(_))));
    }

    #[test]
    fn reports_operator_attribution_and_protection() {
        let operator = software("witness.op");
        let operator_id = operator.key_id();
        let guardian = software("witness.guardian");
        let dual = DualControlSigner::new(operator, guardian);
        assert_eq!(dual.key_id(), operator_id);
        assert_eq!(dual.protection(), KeyProtection::Software);
    }
}
