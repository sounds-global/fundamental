use super::declared_subscription::DeclaredSubscription;
use super::permissions_violation::PermissionsViolation;
use std::sync::Mutex;
use tracing::{error, warn};

/// What this process declared, and which of those the server refused.
///
/// A `SUB` is fire-and-forget, so a refused one arrives only as an async `-ERR` on a connection
/// that stays open. Tracked per subscription and cleared on re-declaration, since NATS never
/// closes the connection over it.
#[derive(Debug, Default)]
pub(crate) struct SubscriptionInventory {
    entries: Mutex<Vec<Entry>>,
}

#[derive(Debug)]
struct Entry {
    declared: DeclaredSubscription,
    /// `true` once the server has refused this subscription and it has not been
    /// re-declared since.
    refused: bool,
}

impl SubscriptionInventory {
    /// Record that this process has asked for `declared`.
    ///
    /// Re-declaring clears any previous refusal for the same subject and group.
    pub(crate) fn declare(&self, declared: DeclaredSubscription) {
        let Ok(mut entries) = self.entries.lock() else {
            warn!("subscription inventory poisoned; readiness cannot see this subscription");
            return;
        };
        if let Some(existing) = entries.iter_mut().find(|entry| entry.declared == declared) {
            existing.refused = false;
            return;
        }
        entries.push(Entry {
            declared,
            refused: false,
        });
    }

    /// Forget a subscription this process has stopped.
    ///
    /// Without this readiness would answer for a subscription nobody holds.
    pub(crate) fn withdraw(&self, declared: &DeclaredSubscription) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|entry| &entry.declared != declared);
        }
    }

    /// Mark the subscription a server refusal names.
    ///
    /// A refusal matching nothing declared is logged and dropped.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) fn refuse(&self, violation: &PermissionsViolation) {
        let Ok(mut entries) = self.entries.lock() else {
            warn!("subscription inventory poisoned; a refusal is not being tracked");
            return;
        };
        let matched = entries.iter_mut().find(|entry| {
            entry.declared.subject == violation.subject
                && entry.declared.queue_group == violation.queue_group
        });
        match matched {
            Some(entry) => {
                error!(
                    subject = %violation.subject,
                    queue_group = ?violation.queue_group,
                    "NATS refused a subscription this service declared; reporting not ready"
                );
                entry.refused = true;
            }
            None => warn!(
                subject = %violation.subject,
                "NATS refused a subscription this service did not declare; ignoring"
            ),
        }
    }

    /// The first refused subscription, if any — what readiness reports.
    pub(crate) fn refused(&self) -> Option<DeclaredSubscription> {
        let entries = self.entries.lock().ok()?;
        entries
            .iter()
            .find(|entry| entry.refused)
            .map(|entry| entry.declared.clone())
    }

    /// Every subscription this process currently holds, in declaration order.
    ///
    /// Refused ones are included. A poisoned lock yields an empty vector, logged, never a
    /// partial one.
    pub(crate) fn declared(&self) -> Vec<DeclaredSubscription> {
        let Ok(entries) = self.entries.lock() else {
            warn!("subscription inventory poisoned; reporting no declared subscriptions");
            return Vec::new();
        };
        entries.iter().map(|entry| entry.declared.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(subject: &str, queue: Option<&str>) -> DeclaredSubscription {
        DeclaredSubscription {
            subject: subject.to_string(),
            queue_group: queue.map(str::to_string),
        }
    }

    fn violation(subject: &str, queue: Option<&str>) -> PermissionsViolation {
        PermissionsViolation {
            subject: subject.to_string(),
            queue_group: queue.map(str::to_string),
        }
    }

    #[test]
    fn a_declared_subscription_is_not_refused_until_the_server_says_so() {
        let inventory = SubscriptionInventory::default();
        inventory.declare(declared("a.b", None));
        assert_eq!(inventory.refused(), None);
    }

    #[test]
    fn a_refusal_marks_the_subscription_it_names() {
        let inventory = SubscriptionInventory::default();
        inventory.declare(declared("a.b", None));
        inventory.declare(declared("c.d", None));
        inventory.refuse(&violation("c.d", None));
        assert_eq!(inventory.refused(), Some(declared("c.d", None)));
    }

    /// A refusal for a queued subscription does not mark the plain one on the same subject.
    #[test]
    fn a_queue_refusal_does_not_mark_a_plain_subscription() {
        let inventory = SubscriptionInventory::default();
        inventory.declare(declared("a.b", None));
        inventory.refuse(&violation("a.b", Some("workers")));
        assert_eq!(inventory.refused(), None);
    }

    /// Re-declaring clears the refusal.
    #[test]
    fn re_declaring_clears_a_previous_refusal() {
        let inventory = SubscriptionInventory::default();
        inventory.declare(declared("a.b", None));
        inventory.refuse(&violation("a.b", None));
        assert!(inventory.refused().is_some());

        inventory.declare(declared("a.b", None));
        assert_eq!(
            inventory.refused(),
            None,
            "a fresh SUB is a fresh question; readiness must be able to recover"
        );
    }

    #[test]
    fn withdrawing_stops_readiness_answering_for_it() {
        let inventory = SubscriptionInventory::default();
        inventory.declare(declared("a.b", None));
        inventory.refuse(&violation("a.b", None));
        inventory.withdraw(&declared("a.b", None));
        assert_eq!(
            inventory.refused(),
            None,
            "a stopped listener must not hold the service unready"
        );
    }

    /// A refusal for something never declared is ignored.
    #[test]
    fn an_unknown_refusal_is_ignored() {
        let inventory = SubscriptionInventory::default();
        inventory.declare(declared("a.b", None));
        inventory.refuse(&violation("someone.else", None));
        assert_eq!(inventory.refused(), None);
    }

    /// The read-back lists what this process asked for, in the order it asked.
    #[test]
    fn every_declaration_is_readable_in_declaration_order() {
        let inventory = SubscriptionInventory::default();
        inventory.declare(declared("a.b", None));
        inventory.declare(declared("c.d", Some("workers")));

        assert_eq!(
            inventory.declared(),
            vec![declared("a.b", None), declared("c.d", Some("workers"))]
        );
    }

    /// A refused subscription is still one this process declared.
    #[test]
    fn a_refused_subscription_is_still_declared() {
        let inventory = SubscriptionInventory::default();
        inventory.declare(declared("a.b", None));
        inventory.refuse(&violation("a.b", None));

        assert_eq!(inventory.declared(), vec![declared("a.b", None)]);
    }

    /// A stopped listener leaves the read-back too.
    #[test]
    fn withdrawing_removes_it_from_the_read_back() {
        let inventory = SubscriptionInventory::default();
        inventory.declare(declared("a.b", None));
        inventory.declare(declared("c.d", None));
        inventory.withdraw(&declared("a.b", None));

        assert_eq!(inventory.declared(), vec![declared("c.d", None)]);
    }

    #[test]
    fn declaring_twice_does_not_duplicate_an_entry() {
        let inventory = SubscriptionInventory::default();
        inventory.declare(declared("a.b", None));
        inventory.declare(declared("a.b", None));
        let entries = inventory.entries.lock().expect("lock");
        assert_eq!(entries.len(), 1);
    }
}
