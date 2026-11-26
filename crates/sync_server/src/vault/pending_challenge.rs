use std::sync::Arc;

use crate::vault::{VaultChallenge, VaultChallengeGauge};

/// A challenge a connection is waiting for an answer to.
///
/// Creating one adds to the gauge and dropping it subtracts.
#[derive(Debug)]
pub(crate) struct PendingChallenge {
    challenge: VaultChallenge,
    gauge: Arc<VaultChallengeGauge>,
}

impl PendingChallenge {
    pub(crate) fn new(challenge: VaultChallenge, gauge: Arc<VaultChallengeGauge>) -> Self {
        gauge.hold();
        Self { challenge, gauge }
    }

    pub(crate) fn challenge(&self) -> &VaultChallenge {
        &self.challenge
    }
}

impl Drop for PendingChallenge {
    fn drop(&mut self) {
        self.gauge.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pending_challenge_is_counted_while_it_lives() {
        let gauge = Arc::new(VaultChallengeGauge::new());
        let pending = PendingChallenge::new(VaultChallenge::new([1; 32], 10), gauge.clone());
        assert_eq!(gauge.pending(), 1);
        assert_eq!(pending.challenge().nonce(), &[1; 32]);
        drop(pending);
        assert_eq!(gauge.pending(), 0);
    }
}
