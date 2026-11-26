use fundamental_keys::{Signature, VaultOwnerId, VerifyingKey, verify_owner_challenge};
use fundamental_sync::VaultError;
use fundamental_sync::wire::VaultAuthRequest;
use subtle::ConstantTimeEq;
use tracing::debug;

use crate::vault::pending_challenge::PendingChallenge;
use crate::vault::{VaultHandshake, VaultPrincipal};

/// Authenticates a vault connection from its answer to the connection's challenge, binding it to
/// a [`VaultPrincipal`].
///
/// The pending challenge is spent before the answer is read, so no signature replays.
///
/// # Errors
/// [`VaultError::OwnerProofFailed`] for every failure alike, so the endpoint is no oracle.
pub fn authenticate_vault(
    handshake: &VaultHandshake,
    answer: &VaultAuthRequest,
    now_ms: i64,
) -> Result<VaultPrincipal, VaultError> {
    let pending = handshake.take();
    let outcome = prove(pending.as_ref(), answer, now_ms)
        .and_then(|principal| handshake.bind(principal).map(|()| principal));
    if outcome.is_err() {
        handshake.record_failure();
    }
    outcome
}

fn prove(
    pending: Option<&PendingChallenge>,
    answer: &VaultAuthRequest,
    now_ms: i64,
) -> Result<VaultPrincipal, VaultError> {
    let Some(challenge) = pending.map(PendingChallenge::challenge) else {
        return refuse("no challenge is pending");
    };
    // Constant-time though the nonce is not secret, so no path's timing depends on it.
    if !bool::from(challenge.nonce().as_slice().ct_eq(&answer.nonce)) {
        return refuse("the nonce is not the pending challenge's");
    }
    if challenge.expires_at_ms() < now_ms {
        return refuse("the challenge has expired");
    }
    let Ok(owner_public_key) = answer.owner_public_key.parse::<VerifyingKey>() else {
        return refuse("the owner key does not parse");
    };
    let Ok(signature) = answer.signature.parse::<Signature>() else {
        return refuse("the signature does not parse");
    };
    if verify_owner_challenge(&owner_public_key, challenge.nonce(), &signature).is_err() {
        return refuse("the signature does not verify");
    }
    Ok(VaultPrincipal::new(VaultOwnerId::from_public_key(
        &owner_public_key,
    )))
}

fn refuse<T>(reason: &'static str) -> Result<T, VaultError> {
    debug!(reason, "vault answer refused");
    Err(VaultError::OwnerProofFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use fundamental_core::Error;
    use fundamental_core::secrets::Secret;
    use fundamental_keys::VaultOwnerKey;

    use crate::vault::VaultChallengeGauge;

    const TTL_MS: i64 = 300_000;

    fn owner_key(seed: u8) -> VaultOwnerKey {
        VaultOwnerKey::from_child_seed(&Secret::new([seed; 32]))
    }

    fn handshake() -> (VaultHandshake, Arc<VaultChallengeGauge>) {
        let gauge = Arc::new(VaultChallengeGauge::new());
        (VaultHandshake::new(gauge.clone()), gauge)
    }

    fn answer(key: &VaultOwnerKey, signer: &VaultOwnerKey, nonce: &[u8; 32]) -> VaultAuthRequest {
        VaultAuthRequest::new(
            1,
            key.public_key().to_string(),
            nonce.to_vec(),
            signer.sign_challenge(nonce).to_string(),
        )
    }

    fn assert_refused(outcome: Result<VaultPrincipal, VaultError>) {
        let error = outcome.expect_err("refused");
        assert!(matches!(error, VaultError::OwnerProofFailed));
        assert_eq!(error.error_code(), "VAULT_003");
        assert_eq!(error.http_status_code(), 401);
    }

    #[test]
    fn a_valid_proof_binds_the_blinded_owner() {
        let (handshake, gauge) = handshake();
        let key = owner_key(0x11);
        let challenge = handshake.issue([0x01; 32], 1_000, TTL_MS).expect("issued");

        let principal =
            authenticate_vault(&handshake, &answer(&key, &key, challenge.nonce()), 1_500)
                .expect("valid proof");
        assert_eq!(principal.owner_id(), &key.owner_id());
        assert_eq!(handshake.principal(), Some(&principal));
        assert_eq!(gauge.pending(), 0);
    }

    #[test]
    fn a_replayed_proof_is_rejected() {
        let (handshake, _) = handshake();
        let key = owner_key(0x22);
        let challenge = handshake.issue([0x02; 32], 0, TTL_MS).expect("issued");
        let proof = answer(&key, &key, challenge.nonce());

        assert!(authenticate_vault(&handshake, &proof, 10).is_ok());
        assert_refused(authenticate_vault(&handshake, &proof, 10));
    }

    #[test]
    fn an_answer_to_a_replaced_challenge_is_refused() {
        let (handshake, _) = handshake();
        let key = owner_key(0x23);
        let replaced = handshake.issue([0x0a; 32], 0, TTL_MS).expect("issued");
        let _current = handshake.issue([0x0b; 32], 0, TTL_MS).expect("issued");

        assert_refused(authenticate_vault(
            &handshake,
            &answer(&key, &key, replaced.nonce()),
            10,
        ));
        assert!(handshake.principal().is_none());
    }

    #[test]
    fn a_wrong_answer_spends_the_current_challenge_too() {
        let (handshake, gauge) = handshake();
        let key = owner_key(0x24);
        let current = handshake.issue([0x0c; 32], 0, TTL_MS).expect("issued");

        assert_refused(authenticate_vault(
            &handshake,
            &answer(&key, &key, &[0x0d; 32]),
            10,
        ));
        assert_eq!(gauge.pending(), 0);
        assert_refused(authenticate_vault(
            &handshake,
            &answer(&key, &key, current.nonce()),
            10,
        ));
    }

    #[test]
    fn an_unparsable_answer_spends_the_challenge() {
        let (handshake, gauge) = handshake();
        let key = owner_key(0x25);
        let challenge = handshake.issue([0x0e; 32], 0, TTL_MS).expect("issued");

        let garbled = VaultAuthRequest::new(1, "not a key", challenge.nonce().to_vec(), "nor this");
        assert_refused(authenticate_vault(&handshake, &garbled, 10));
        assert_eq!(gauge.pending(), 0);
        assert_refused(authenticate_vault(
            &handshake,
            &answer(&key, &key, challenge.nonce()),
            10,
        ));
    }

    #[test]
    fn an_unparsable_signature_is_refused() {
        let (handshake, _) = handshake();
        let key = owner_key(0x26);
        let challenge = handshake.issue([0x0f; 32], 0, TTL_MS).expect("issued");
        let garbled = VaultAuthRequest::new(
            1,
            key.public_key().to_string(),
            challenge.nonce().to_vec(),
            "not a signature",
        );
        assert_refused(authenticate_vault(&handshake, &garbled, 10));
    }

    #[test]
    fn a_wrong_key_cannot_answer_the_challenge() {
        let (handshake, _) = handshake();
        let owner = owner_key(0x33);
        let attacker = owner_key(0x44);
        let challenge = handshake.issue([0x03; 32], 0, TTL_MS).expect("issued");

        assert_refused(authenticate_vault(
            &handshake,
            &answer(&owner, &attacker, challenge.nonce()),
            10,
        ));
    }

    #[test]
    fn an_expired_challenge_is_rejected() {
        let (handshake, _) = handshake();
        let key = owner_key(0x55);
        let challenge = handshake.issue([0x04; 32], 0, 100).expect("issued");

        assert_refused(authenticate_vault(
            &handshake,
            &answer(&key, &key, challenge.nonce()),
            1_000,
        ));
    }

    #[test]
    fn a_wrong_length_nonce_is_rejected() {
        let (handshake, _) = handshake();
        let key = owner_key(0x66);
        handshake.issue([0u8; 32], 0, TTL_MS).expect("issued");
        let short = VaultAuthRequest::new(
            1,
            key.public_key().to_string(),
            vec![0u8; 16],
            key.sign_challenge(&[0u8; 32]).to_string(),
        );
        assert_refused(authenticate_vault(&handshake, &short, 10));
    }

    #[test]
    fn an_answer_with_no_challenge_is_refused() {
        let (handshake, _) = handshake();
        let key = owner_key(0x77);
        assert_refused(authenticate_vault(
            &handshake,
            &answer(&key, &key, &[0x05; 32]),
            10,
        ));
    }

    #[test]
    fn a_nonce_issued_to_another_connection_cannot_answer_this_one() {
        let (alice, _) = handshake();
        let (mallory, _) = handshake();
        let key = owner_key(0x88);
        let alices = alice.issue([0x06; 32], 0, TTL_MS).expect("issued");
        mallory.issue([0x07; 32], 0, TTL_MS).expect("issued");

        assert_refused(authenticate_vault(
            &mallory,
            &answer(&key, &key, alices.nonce()),
            10,
        ));
    }

    #[test]
    fn a_bound_connection_cannot_authenticate_again() {
        let (handshake, _) = handshake();
        let first = owner_key(0x99);
        let second = owner_key(0x9a);
        let challenge = handshake.issue([0x08; 32], 0, TTL_MS).expect("issued");
        authenticate_vault(&handshake, &answer(&first, &first, challenge.nonce()), 10)
            .expect("first proof");

        let again = handshake.issue([0x09; 32], 0, TTL_MS).expect("issued");
        assert_refused(authenticate_vault(
            &handshake,
            &answer(&second, &second, again.nonce()),
            10,
        ));
        assert_eq!(
            handshake.principal().map(VaultPrincipal::owner_id),
            Some(&first.owner_id())
        );
    }

    #[test]
    fn three_wrong_answers_refuse_the_next_challenge() {
        let (handshake, _) = handshake();
        let key = owner_key(0xab);
        for _ in 0..3 {
            handshake.issue([0x10; 32], 0, TTL_MS).expect("issued");
            assert_refused(authenticate_vault(
                &handshake,
                &answer(&key, &key, &[0x11; 32]),
                10,
            ));
        }
        assert!(matches!(
            handshake.issue([0x10; 32], 0, TTL_MS),
            Err(VaultError::OwnerProofFailed)
        ));
    }
}
