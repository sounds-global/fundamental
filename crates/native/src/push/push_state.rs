use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::{PlatformHooks, PushMessage, PushRegistrar, PushRegistration};

/// How many messages are held while the app's hooks are still starting.
///
/// A cold start delivers one notification; anything past this is logged and dropped.
const PENDING_CAPACITY: usize = 8;

/// The managed handle to the push seam, an `Arc` a spawned task can own.
pub(crate) struct PushState(pub(crate) Arc<PushInner>);

impl PushState {
    /// Wires the app's registrar and hooks into a fresh seam.
    pub(crate) fn new(
        registrar: Arc<dyn PushRegistrar>,
        hooks: Arc<[Box<dyn PlatformHooks>]>,
    ) -> Self {
        Self(Arc::new(PushInner {
            registrar,
            hooks,
            dedup: Mutex::new(PushDedup::default()),
            pending: Mutex::new(PendingMessages::default()),
        }))
    }
}

pub(crate) struct PushInner {
    pub(crate) registrar: Arc<dyn PushRegistrar>,
    pub(crate) hooks: Arc<[Box<dyn PlatformHooks>]>,
    dedup: Mutex<PushDedup>,
    pending: Mutex<PendingMessages>,
}

impl PushInner {
    /// Decides whether `registration` needs to reach the registrar.
    pub(crate) fn offer(&self, registration: &PushRegistration) -> PushDecision {
        lock(&self.dedup).offer(registration)
    }

    /// Records the outcome of the registration started at `epoch`.
    pub(crate) fn settle(&self, epoch: u64, accepted: bool) {
        lock(&self.dedup).settle(epoch, accepted);
    }

    /// Forgets the device's registration, cancelling any in-flight commit.
    pub(crate) fn forget(&self) {
        lock(&self.dedup).forget();
    }

    /// Takes the registration whose recording failed, if one is waiting.
    pub(crate) fn retryable(&self) -> Option<PushRegistration> {
        lock(&self.dedup).retryable()
    }

    /// Offers an inbound message, which is delivered now or held until hooks start.
    pub(crate) fn offer_message(&self, message: PushMessage) -> PendingOutcome {
        lock(&self.pending).offer(message)
    }

    /// Marks the hooks ready and returns everything that arrived before they were.
    pub(crate) fn release_pending(&self) -> Vec<PushMessage> {
        lock(&self.pending).release()
    }
}

/// Locks one of the seam's guards, treating a poisoned lock as usable.
///
/// The values are plain bookkeeping that a panic leaves consistent.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PushDecision {
    /// The registrar already holds this registration, or is being told it now.
    Suppress,
    /// Hand it to the registrar; report the result under this epoch.
    Deliver {
        /// The generation this delivery belongs to.
        epoch: u64,
    },
}

/// The registration bookkeeping; a registration is remembered only once the registrar accepts it.
#[derive(Debug, Default)]
struct PushDedup {
    accepted: Option<PushRegistration>,
    in_flight: Option<PushRegistration>,
    failed: Option<PushRegistration>,
    epoch: u64,
}

impl PushDedup {
    /// Suppresses a repeat, and otherwise opens a new generation for delivery.
    fn offer(&mut self, registration: &PushRegistration) -> PushDecision {
        if self.accepted.as_ref() == Some(registration)
            || self.in_flight.as_ref() == Some(registration)
        {
            return PushDecision::Suppress;
        }
        self.epoch = self.epoch.wrapping_add(1);
        self.in_flight = Some(registration.clone());
        self.failed = None;
        PushDecision::Deliver { epoch: self.epoch }
    }

    /// Commits a generation's outcome, if it is still the current one.
    ///
    /// A stale generation, for a token since rotated away, never becomes the record.
    fn settle(&mut self, epoch: u64, accepted: bool) {
        if epoch != self.epoch {
            return;
        }
        let settled = self.in_flight.take();
        if accepted {
            self.accepted = settled;
            self.failed = None;
        } else {
            self.accepted = None;
            self.failed = settled;
        }
    }

    /// Drops everything, including whatever is in flight.
    ///
    /// The epoch moves, so an in-flight registration cannot undo the revocation.
    fn forget(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.in_flight = None;
        self.accepted = None;
        self.failed = None;
    }

    /// Takes the failed registration so it can be offered again.
    fn retryable(&mut self) -> Option<PushRegistration> {
        self.failed.take()
    }
}

#[derive(Debug)]
pub(crate) enum PendingOutcome {
    /// Hooks are up; deliver it now.
    Deliver(PushMessage),
    /// Held until the hooks finish starting.
    Queued,
    /// The queue is full and the message was refused.
    Dropped,
}

/// Messages that arrived before the app's hooks were running.
///
/// A notification tap reaches the platform before setup runs.
#[derive(Debug, Default)]
struct PendingMessages {
    ready: bool,
    queued: VecDeque<PushMessage>,
}

impl PendingMessages {
    /// Delivers now, or holds, refusing the newest first so the launching notification survives.
    fn offer(&mut self, message: PushMessage) -> PendingOutcome {
        if self.ready {
            return PendingOutcome::Deliver(message);
        }
        if self.queued.len() >= PENDING_CAPACITY {
            return PendingOutcome::Dropped;
        }
        self.queued.push_back(message);
        PendingOutcome::Queued
    }

    /// Opens the gate and hands back what was held, oldest first.
    fn release(&mut self) -> Vec<PushMessage> {
        self.ready = true;
        self.queued.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::secrets::SecretString;

    use super::{PENDING_CAPACITY, PendingMessages, PendingOutcome, PushDecision, PushDedup};
    use crate::{PushDelivery, PushMessage, PushProvider, PushRegistration};

    fn token(value: &str) -> PushRegistration {
        PushRegistration::new(PushProvider::Apns, SecretString::new(value.to_owned()))
    }

    fn epoch_of(decision: PushDecision) -> u64 {
        match decision {
            PushDecision::Deliver { epoch } => epoch,
            PushDecision::Suppress => panic!("expected a delivery, not a suppression"),
        }
    }

    fn message(id: &str) -> PushMessage {
        PushMessage::from_json(PushDelivery::Foreground, &format!(r#"{{"id":"{id}"}}"#))
            .expect("an object payload is valid")
    }

    #[test]
    fn the_first_registration_of_a_run_is_delivered() {
        let mut dedup = PushDedup::default();
        assert_eq!(epoch_of(dedup.offer(&token("a"))), 1);
    }

    #[test]
    fn an_accepted_registration_is_not_delivered_again() {
        let mut dedup = PushDedup::default();
        let epoch = epoch_of(dedup.offer(&token("a")));
        dedup.settle(epoch, true);
        assert_eq!(
            dedup.offer(&token("a")),
            PushDecision::Suppress,
            "the platform re-offers the same token on every launch; the backend must not be rewritten"
        );
    }

    #[test]
    fn a_rotation_is_always_delivered() {
        let mut dedup = PushDedup::default();
        let epoch = epoch_of(dedup.offer(&token("a")));
        dedup.settle(epoch, true);
        assert_eq!(epoch_of(dedup.offer(&token("b"))), 2);
    }

    #[test]
    fn concurrent_identical_offers_produce_one_delivery() {
        let mut dedup = PushDedup::default();
        epoch_of(dedup.offer(&token("a")));
        assert_eq!(
            dedup.offer(&token("a")),
            PushDecision::Suppress,
            "a second delivery arriving while the first is in flight must not double-register"
        );
    }

    #[test]
    fn a_rejected_registration_is_not_remembered_and_can_be_retried() {
        let mut dedup = PushDedup::default();
        let epoch = epoch_of(dedup.offer(&token("a")));
        dedup.settle(epoch, false);
        assert_eq!(
            dedup.retryable(),
            Some(token("a")),
            "a registration the registrar refused is owed another attempt"
        );
        assert_eq!(epoch_of(dedup.offer(&token("a"))), 2);
    }

    #[test]
    fn the_retry_is_offered_only_once() {
        let mut dedup = PushDedup::default();
        let epoch = epoch_of(dedup.offer(&token("a")));
        dedup.settle(epoch, false);
        assert!(dedup.retryable().is_some());
        assert_eq!(dedup.retryable(), None);
    }

    #[test]
    fn a_superseded_registration_does_not_become_the_record() {
        let mut dedup = PushDedup::default();
        let slow = epoch_of(dedup.offer(&token("a")));
        let fast = epoch_of(dedup.offer(&token("b")));
        dedup.settle(fast, true);
        dedup.settle(slow, true);

        assert_eq!(
            dedup.offer(&token("b")),
            PushDecision::Suppress,
            "the rotated token is the record and must stay suppressed"
        );
        assert_eq!(
            epoch_of(dedup.offer(&token("a"))),
            3,
            "the superseded token must not have been recorded by its late completion"
        );
    }

    #[test]
    fn a_superseded_failure_does_not_schedule_a_retry() {
        let mut dedup = PushDedup::default();
        let slow = epoch_of(dedup.offer(&token("a")));
        let fast = epoch_of(dedup.offer(&token("b")));
        dedup.settle(fast, true);
        dedup.settle(slow, false);
        assert_eq!(
            dedup.retryable(),
            None,
            "a token the platform has moved on from must not be retried"
        );
    }

    #[test]
    fn forgetting_cancels_a_commit_that_is_still_in_flight() {
        let mut dedup = PushDedup::default();
        let epoch = epoch_of(dedup.offer(&token("a")));
        dedup.forget();
        dedup.settle(epoch, true);
        assert_eq!(
            epoch_of(dedup.offer(&token("a"))),
            3,
            "a revocation must not be undone by the registration it interrupted"
        );
    }

    #[test]
    fn forgetting_clears_an_accepted_registration() {
        let mut dedup = PushDedup::default();
        let epoch = epoch_of(dedup.offer(&token("a")));
        dedup.settle(epoch, true);
        dedup.forget();
        assert_eq!(
            epoch_of(dedup.offer(&token("a"))),
            3,
            "after a revocation the same token is a new registration"
        );
        assert_eq!(dedup.retryable(), None);
    }

    #[test]
    fn messages_arriving_before_the_hooks_are_held_in_order() {
        let mut pending = PendingMessages::default();
        assert!(matches!(
            pending.offer(message("1")),
            PendingOutcome::Queued
        ));
        assert!(matches!(
            pending.offer(message("2")),
            PendingOutcome::Queued
        ));
        let released = pending.release();
        assert_eq!(released, vec![message("1"), message("2")]);
    }

    #[test]
    fn messages_arriving_after_the_hooks_are_delivered_straight_through() {
        let mut pending = PendingMessages::default();
        assert!(pending.release().is_empty());
        assert!(matches!(
            pending.offer(message("1")),
            PendingOutcome::Deliver(_)
        ));
    }

    #[test]
    fn the_cold_start_queue_refuses_rather_than_grows() {
        let mut pending = PendingMessages::default();
        for index in 0..PENDING_CAPACITY {
            assert!(matches!(
                pending.offer(message(&index.to_string())),
                PendingOutcome::Queued
            ));
        }
        assert!(matches!(
            pending.offer(message("overflow")),
            PendingOutcome::Dropped
        ));
        assert_eq!(
            pending.release().first(),
            Some(&message("0")),
            "the launching notification is the one worth keeping"
        );
    }
}
