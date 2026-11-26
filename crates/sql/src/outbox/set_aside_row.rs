use chrono::{DateTime, TimeDelta, Utc};
use fundamental_core::outbox::{OutboxColumns, RefusalKind, SetAsideBucket};
use fundamental_encryption::KeyManager;

use super::ClaimedKey;

/// How far ahead of the reader's clock a claim's instant may be before it is not believed.
pub(crate) const CLOCK_SKEW: TimeDelta = TimeDelta::minutes(5);

/// One set-aside row as an inventory reads it: its whole key and the columns a claim writes.
pub(crate) struct SetAsideRow {
    pub(crate) key: ClaimedKey,
    pub(crate) at: DateTime<Utc>,
    pub(crate) reason: Option<String>,
    pub(crate) seal: Option<Vec<u8>>,
}

impl SetAsideRow {
    /// The parts a set-aside seal covers, built here for both the claim and the inventory: key, instant in microseconds, and reason.
    pub(crate) fn seal_parts(
        key: &ClaimedKey,
        at_micros: i64,
        kind: RefusalKind,
    ) -> Vec<(&'static str, Option<Vec<u8>>)> {
        let mut parts = key.seal_parts();
        parts.push((
            OutboxColumns::SET_ASIDE_AT,
            Some(at_micros.to_le_bytes().to_vec()),
        ));
        parts.push((
            OutboxColumns::SET_ASIDE_REASON,
            Some(kind.code().as_bytes().to_vec()),
        ));
        parts
    }

    /// The bucket this row counts under at `now`, and whether its seal was checked here.
    ///
    /// Under its stated reason only when a claim provably wrote it; a seal under a key version
    /// this process lacks counts under its reason, unchecked.
    pub(crate) fn bucket(
        &self,
        manager: &KeyManager,
        table: &'static str,
        now: DateTime<Utc>,
    ) -> (SetAsideBucket, bool) {
        let Some(kind) = self.reason.as_deref().and_then(RefusalKind::from_code) else {
            return (SetAsideBucket::Unexplained, true);
        };
        let Some(seal) = &self.seal else {
            return (SetAsideBucket::Unexplained, true);
        };
        if now
            .checked_add_signed(CLOCK_SKEW)
            .is_none_or(|latest| self.at > latest)
        {
            return (SetAsideBucket::Unexplained, true);
        }
        let parts = Self::seal_parts(&self.key, self.at.timestamp_micros(), kind);
        match manager.check_outbox_set_aside(seal, table, &parts) {
            Ok(()) => (SetAsideBucket::Reason(kind), true),
            Err(error) if error.refusal() == Some(RefusalKind::KeyNotHeld) => {
                (SetAsideBucket::Reason(kind), false)
            }
            Err(_) => (SetAsideBucket::Unexplained, true),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeDelta, Utc};
    use fundamental_core::outbox::{RefusalKind, SetAsideBucket};
    use fundamental_encryption::{EncryptionAlgorithm, KeyManager};

    use super::{ClaimedKey, SetAsideRow};

    const TABLE: &str = "held_events";

    fn manager(versions: &[(u32, u8)]) -> KeyManager {
        versions
            .iter()
            .fold(KeyManager::builder(), |builder, (version, material)| {
                builder.with_key_version(
                    KeyManager::OUTBOX_SEAL_KEY,
                    *version,
                    &[*material; 32],
                    EncryptionAlgorithm::ChaCha20Poly1305,
                )
            })
            .build()
            .expect("an outbox seal key")
    }

    fn key() -> ClaimedKey {
        ClaimedKey::from_parts(vec![(
            "id",
            "UUID",
            "7f1c0000-0000-0000-0000-000000000000".to_owned(),
        )])
    }

    fn sealed(sealer: &KeyManager, kind: RefusalKind, reason: &str) -> SetAsideRow {
        let at = Utc::now();
        let seal = sealer
            .seal_outbox_set_aside(
                TABLE,
                &SetAsideRow::seal_parts(&key(), at.timestamp_micros(), kind),
            )
            .expect("seals");
        SetAsideRow {
            key: key(),
            at: chrono::DateTime::from_timestamp_micros(at.timestamp_micros()).expect("in range"),
            reason: Some(reason.to_owned()),
            seal: Some(seal.to_vec()),
        }
    }

    #[test]
    fn a_set_aside_a_claim_sealed_counts_under_its_reason() {
        let keys = manager(&[(1, 1)]);
        let row = sealed(&keys, RefusalKind::EnvelopeRefused, "envelope_refused");
        assert_eq!(
            row.bucket(&keys, TABLE, Utc::now()),
            (SetAsideBucket::Reason(RefusalKind::EnvelopeRefused), true)
        );
    }

    #[test]
    fn a_set_aside_without_a_seal_is_unexplained() {
        let keys = manager(&[(1, 1)]);
        let mut row = sealed(&keys, RefusalKind::EnvelopeRefused, "envelope_refused");
        row.seal = None;
        assert_eq!(
            row.bucket(&keys, TABLE, Utc::now()).0,
            SetAsideBucket::Unexplained
        );
    }

    #[test]
    fn a_reason_edited_after_the_claim_is_unexplained() {
        let keys = manager(&[(1, 1)]);
        let row = sealed(&keys, RefusalKind::EnvelopeRefused, "value_refused");
        assert_eq!(
            row.bucket(&keys, TABLE, Utc::now()).0,
            SetAsideBucket::Unexplained
        );
    }

    #[test]
    fn a_reason_the_framework_never_writes_is_unexplained() {
        let keys = manager(&[(1, 1)]);
        let row = sealed(&keys, RefusalKind::EnvelopeRefused, "tampered");
        assert_eq!(
            row.bucket(&keys, TABLE, Utc::now()).0,
            SetAsideBucket::Unexplained
        );
    }

    #[test]
    fn a_set_aside_stamped_in_the_future_is_unexplained() {
        let keys = manager(&[(1, 1)]);
        let row = sealed(&keys, RefusalKind::EnvelopeRefused, "envelope_refused");
        let long_ago = Utc::now() - TimeDelta::hours(1);
        assert_eq!(
            row.bucket(&keys, TABLE, long_ago).0,
            SetAsideBucket::Unexplained,
            "an instant an hour past the reader's clock was not written by a claim"
        );
    }

    #[test]
    fn a_seal_under_a_version_this_process_lacks_counts_under_its_reason_unchecked() {
        let rotated = manager(&[(1, 1), (2, 2)]);
        let row = sealed(&rotated, RefusalKind::ValueRefused, "value_refused");
        assert_eq!(
            row.bucket(&manager(&[(1, 1)]), TABLE, Utc::now()),
            (SetAsideBucket::Reason(RefusalKind::ValueRefused), false)
        );
    }
}
