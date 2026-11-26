use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use fundamental_sync::VaultError;
use tracing::{debug, warn};

use crate::vault::pending_challenge::PendingChallenge;
use crate::vault::{VaultChallenge, VaultChallengeGauge, VaultPrincipal};

/// Wrong answers a connection may give before it is refused further challenges; each costs a
/// signature verification.
const MAX_FAILED_ANSWERS: u8 = 3;
/// The window replacements are counted over for the high-rate warning.
const REPLACEMENT_WINDOW_MS: i64 = 60_000;
/// Replacements within one window that earn the connection one warning.
const REPLACEMENTS_WORTH_A_WARNING: u32 = 32;
/// Store operations a proven owner may make before it is admitted.
const MAX_UNBOUND_OPERATIONS: u32 = 4;

/// The vault handshake of one connection: the one challenge it is waiting to have answered,
/// and the blinded owner it proved.
///
/// Dropped with the connection, so pending challenges are bounded by open connections. It does
/// not admit the connection: the hub binds the owner only once the store holds a row under it.
#[derive(Debug)]
pub struct VaultHandshake {
    gauge: Arc<VaultChallengeGauge>,
    attempt: Mutex<Attempt>,
    principal: OnceLock<VaultPrincipal>,
    probed: Mutex<BTreeSet<String>>,
    unbound_operations: AtomicU32,
}

#[derive(Debug, Default)]
struct Attempt {
    pending: Option<PendingChallenge>,
    replacements: ReplacementWindow,
    failed_answers: u8,
}

#[derive(Debug, Default)]
struct ReplacementWindow {
    started_at_ms: i64,
    replaced: u32,
    warned: bool,
}

impl ReplacementWindow {
    /// Counts one replacement at `now_ms`, returning the count the first time a window
    /// reaches the warning threshold.
    fn record(&mut self, now_ms: i64) -> Option<u32> {
        if now_ms.saturating_sub(self.started_at_ms) >= REPLACEMENT_WINDOW_MS {
            *self = Self {
                started_at_ms: now_ms,
                replaced: 0,
                warned: false,
            };
        }
        self.replaced = self.replaced.saturating_add(1);
        if !self.warned && self.replaced >= REPLACEMENTS_WORTH_A_WARNING {
            self.warned = true;
            return Some(self.replaced);
        }
        None
    }
}

impl VaultHandshake {
    /// Creates a connection's handshake, counting its challenges on `gauge`.
    #[must_use]
    pub fn new(gauge: Arc<VaultChallengeGauge>) -> Self {
        Self {
            gauge,
            attempt: Mutex::new(Attempt::default()),
            principal: OnceLock::new(),
            probed: Mutex::new(BTreeSet::new()),
            unbound_operations: AtomicU32::new(0),
        }
    }

    /// Issues the connection's challenge, replacing any still pending.
    ///
    /// # Errors
    /// [`VaultError::OwnerProofFailed`] once the connection has answered wrongly
    /// `MAX_FAILED_ANSWERS` times.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn issue(
        &self,
        nonce: [u8; 32],
        now_ms: i64,
        ttl_ms: i64,
    ) -> Result<VaultChallenge, VaultError> {
        let mut attempt = self.lock();
        if attempt.failed_answers >= MAX_FAILED_ANSWERS {
            debug!(
                failed_answers = attempt.failed_answers,
                "vault challenge refused after failed answers"
            );
            return Err(VaultError::OwnerProofFailed);
        }
        let challenge = VaultChallenge::new(nonce, now_ms.saturating_add(ttl_ms));
        let fresh = PendingChallenge::new(challenge, self.gauge.clone());
        if attempt.pending.replace(fresh).is_some() {
            self.gauge.count_replacement();
            // At `debug`: any anonymous connection can reach this.
            if let Some(replaced) = attempt.replacements.record(now_ms) {
                debug!(
                    replaced,
                    window_ms = REPLACEMENT_WINDOW_MS,
                    "vault challenges replaced at a high rate on one connection"
                );
            }
        }
        Ok(challenge)
    }

    /// The blinded owner this connection proved, once it has.
    #[must_use]
    pub fn principal(&self) -> Option<&VaultPrincipal> {
        self.principal.get()
    }

    /// Takes the pending challenge out of the connection, so the answer in hand spends it.
    pub(crate) fn take(&self) -> Option<PendingChallenge> {
        self.lock().pending.take()
    }

    /// Records a wrong answer.
    pub(crate) fn record_failure(&self) {
        let mut attempt = self.lock();
        attempt.failed_answers = attempt.failed_answers.saturating_add(1);
        // At `debug`: every wrong answer was already refused and logged.
        if attempt.failed_answers == MAX_FAILED_ANSWERS {
            debug!(
                failed_answers = attempt.failed_answers,
                "vault connection refused further challenges after failed answers"
            );
        }
    }

    /// Whether the store has already answered for this owner in `slice`.
    pub(crate) fn probed(&self, slice: &str) -> bool {
        self.probed_slices().contains(slice)
    }

    /// Records that the store answered for this owner in `slice`.
    pub(crate) fn record_probe(&self, slice: &str) {
        self.probed_slices().insert(slice.to_owned());
    }

    /// Counts one store operation before admission, returning whether it is within the limit.
    pub(crate) fn count_unbound_operation(&self) -> bool {
        self.unbound_operations.fetch_add(1, Ordering::Relaxed) < MAX_UNBOUND_OPERATIONS
    }

    /// Records the owner the connection proved, which scopes it from then on.
    ///
    /// # Errors
    /// [`VaultError::OwnerProofFailed`] if the connection is already bound: a connection
    /// acts as one owner for its whole life.
    pub(crate) fn bind(&self, principal: VaultPrincipal) -> Result<(), VaultError> {
        self.principal.set(principal).map_err(|_| {
            debug!("vault answer refused: the connection is already bound");
            VaultError::OwnerProofFailed
        })
    }

    fn probed_slices(&self) -> MutexGuard<'_, BTreeSet<String>> {
        self.probed.lock().unwrap_or_else(|poisoned| {
            warn!("vault probe lock was poisoned by a panic; recovering its slices");
            poisoned.into_inner()
        })
    }

    /// Locks the attempt, recovering a poisoned lock: every field is valid after any partial update.
    fn lock(&self) -> MutexGuard<'_, Attempt> {
        self.attempt.lock().unwrap_or_else(|poisoned| {
            warn!("vault handshake lock was poisoned by a panic; recovering its state");
            poisoned.into_inner()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error;
    use fundamental_core::secrets::Secret;
    use fundamental_keys::VaultOwnerKey;

    const TTL_MS: i64 = 60_000;

    fn handshake() -> (VaultHandshake, Arc<VaultChallengeGauge>) {
        let gauge = Arc::new(VaultChallengeGauge::new());
        (VaultHandshake::new(gauge.clone()), gauge)
    }

    fn principal(seed: u8) -> VaultPrincipal {
        VaultPrincipal::new(VaultOwnerKey::from_child_seed(&Secret::new([seed; 32])).owner_id())
    }

    #[test]
    fn ten_thousand_challenges_leave_one_pending() {
        let (handshake, gauge) = handshake();
        for index in 0..10_000u32 {
            let mut nonce = [0u8; 32];
            nonce[..4].copy_from_slice(&index.to_be_bytes());
            handshake
                .issue(nonce, i64::from(index), TTL_MS)
                .expect("issued");
        }
        assert_eq!(gauge.pending(), 1);
        assert_eq!(gauge.replaced(), 9_999);
        let pending = handshake.take().expect("the newest challenge");
        let mut newest = [0u8; 32];
        newest[..4].copy_from_slice(&9_999u32.to_be_bytes());
        assert_eq!(pending.challenge().nonce(), &newest);
    }

    #[test]
    fn dropping_the_handshake_releases_its_challenge() {
        let (handshake, gauge) = handshake();
        handshake.issue([1; 32], 0, TTL_MS).expect("issued");
        assert_eq!(gauge.pending(), 1);
        drop(handshake);
        assert_eq!(gauge.pending(), 0);
    }

    #[test]
    fn taking_the_challenge_spends_it() {
        let (handshake, gauge) = handshake();
        handshake.issue([2; 32], 0, TTL_MS).expect("issued");
        let taken = handshake.take();
        assert!(taken.is_some());
        assert!(handshake.take().is_none());
        drop(taken);
        assert_eq!(gauge.pending(), 0);
    }

    #[test]
    fn the_challenge_expires_ttl_after_it_is_issued() {
        let (handshake, _) = handshake();
        let challenge = handshake.issue([3; 32], 1_000, TTL_MS).expect("issued");
        assert_eq!(challenge.expires_at_ms(), 61_000);
    }

    #[test]
    fn three_failed_answers_end_the_connection_s_challenges() {
        let (handshake, gauge) = handshake();
        for _ in 0..MAX_FAILED_ANSWERS {
            handshake.issue([4; 32], 0, TTL_MS).expect("issued");
            drop(handshake.take());
            handshake.record_failure();
        }
        let refused = handshake.issue([4; 32], 0, TTL_MS).unwrap_err();
        assert!(matches!(refused, VaultError::OwnerProofFailed));
        assert_eq!(refused.error_code(), "VAULT_003");
        assert_eq!(gauge.pending(), 0);
    }

    #[test]
    fn two_failed_answers_still_leave_a_challenge() {
        let (handshake, _) = handshake();
        handshake.record_failure();
        handshake.record_failure();
        assert!(handshake.issue([5; 32], 0, TTL_MS).is_ok());
    }

    #[test]
    fn the_store_is_probed_once_per_slice() {
        let (handshake, _) = handshake();
        assert!(!handshake.probed("mine"));
        handshake.record_probe("mine");
        assert!(handshake.probed("mine"));
        assert!(
            !handshake.probed("shared"),
            "another slice is its own question"
        );
    }

    #[test]
    fn an_unbound_owner_has_a_handful_of_operations() {
        let (handshake, _) = handshake();
        let allowed = (0..10)
            .filter(|_| handshake.count_unbound_operation())
            .count();
        assert_eq!(allowed, MAX_UNBOUND_OPERATIONS as usize);
    }

    #[test]
    fn a_bound_connection_cannot_bind_again() {
        let (handshake, _) = handshake();
        handshake.bind(principal(0x11)).expect("first bind");
        let refused = handshake.bind(principal(0x22)).unwrap_err();
        assert!(matches!(refused, VaultError::OwnerProofFailed));
        assert_eq!(
            handshake.principal().map(VaultPrincipal::owner_id),
            Some(principal(0x11).owner_id())
        );
    }

    #[test]
    fn the_replacement_window_warns_once_per_window() {
        let mut window = ReplacementWindow::default();
        let warnings: Vec<u32> = (0..100)
            .filter_map(|_| window.record(REPLACEMENT_WINDOW_MS))
            .collect();
        assert_eq!(warnings, vec![REPLACEMENTS_WORTH_A_WARNING]);
        let next_window = (0..REPLACEMENTS_WORTH_A_WARNING)
            .filter_map(|_| window.record(2 * REPLACEMENT_WINDOW_MS))
            .collect::<Vec<_>>();
        assert_eq!(next_window, vec![REPLACEMENTS_WORTH_A_WARNING]);
    }

    #[test]
    fn replacements_below_the_threshold_do_not_warn() {
        let mut window = ReplacementWindow::default();
        let warnings = (1..REPLACEMENTS_WORTH_A_WARNING)
            .filter_map(|_| window.record(REPLACEMENT_WINDOW_MS))
            .count();
        assert_eq!(warnings, 0);
    }
}
