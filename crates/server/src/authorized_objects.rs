use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

/// The objects a request was authorized on, as `(namespace, relation, canonical id)`.
#[derive(Clone, Default)]
pub struct AuthorizedObjects {
    objects: Arc<Mutex<Vec<Grant>>>,
}

/// One allowed check: the caller holds `relation` on `id` in `namespace`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Grant {
    namespace: String,
    relation: String,
    id: String,
}

impl AuthorizedObjects {
    /// Records that the caller was authorized at `relation` on `id` in `namespace`; for an authorizing middleware only.
    pub fn record(&self, namespace: &str, relation: &str, id: &str) {
        self.objects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Grant {
                namespace: namespace.to_owned(),
                relation: relation.to_owned(),
                id: id.to_owned(),
            });
    }

    /// Whether `id` was authorized in `namespace`, at any relation.
    #[must_use]
    pub fn contains(&self, namespace: &str, id: &str) -> bool {
        self.objects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|grant| grant.namespace == namespace && grant.id == id)
    }

    /// The one object authorized in `namespace`, or `None` if none or two were.
    #[must_use]
    pub fn only_in(&self, namespace: &str) -> Option<String> {
        let objects = self.objects.lock().unwrap_or_else(PoisonError::into_inner);
        single_id(objects.iter().filter(|grant| grant.namespace == namespace))
    }

    /// The one object authorized at exactly `relation` in `namespace`, if it is the only one recorded there.
    #[must_use]
    pub fn granted(&self, namespace: &str, relation: &str) -> Option<String> {
        let objects = self.objects.lock().unwrap_or_else(PoisonError::into_inner);
        let in_namespace = || objects.iter().filter(|grant| grant.namespace == namespace);
        let id = single_id(in_namespace())?;
        in_namespace()
            .any(|grant| grant.relation == relation)
            .then_some(id)
    }
}

/// The id every grant names, or `None` when there are none or they disagree.
fn single_id<'a>(mut grants: impl Iterator<Item = &'a Grant>) -> Option<String> {
    let first = grants.next()?;
    grants
        .all(|grant| grant.id == first.id)
        .then(|| first.id.clone())
}

impl fmt::Debug for AuthorizedObjects {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuthorizedObjects")
    }
}

#[cfg(test)]
mod tests {
    use super::AuthorizedObjects;

    #[test]
    fn one_object_per_namespace_or_none() {
        let objects = AuthorizedObjects::default();
        assert_eq!(objects.only_in("team"), None);
        objects.record("team", "member", "7");
        objects.record("message", "reader", "9");
        let shared = objects.clone();
        assert_eq!(shared.only_in("team").as_deref(), Some("7"));
        objects.record("team", "admin", "7");
        assert_eq!(
            objects.only_in("team").as_deref(),
            Some("7"),
            "the same id twice is one"
        );
        objects.record("team", "member", "8");
        assert_eq!(
            objects.only_in("team"),
            None,
            "two objects leave no single answer"
        );
    }

    #[test]
    fn granted_needs_the_exact_relation() {
        let objects = AuthorizedObjects::default();
        objects.record("team", "member", "7");
        assert_eq!(objects.granted("team", "member").as_deref(), Some("7"));
        assert_eq!(
            objects.granted("team", "admin"),
            None,
            "a member check says nothing about admin"
        );
        assert_eq!(objects.granted("project", "member"), None);
        objects.record("team", "admin", "7");
        assert_eq!(objects.granted("team", "admin").as_deref(), Some("7"));
    }

    #[test]
    fn granted_refuses_a_namespace_holding_two_objects() {
        let objects = AuthorizedObjects::default();
        objects.record("team", "admin", "7");
        objects.record("team", "member", "8");
        assert_eq!(
            objects.granted("team", "admin"),
            None,
            "another object in the namespace leaves no single answer"
        );
    }
}
