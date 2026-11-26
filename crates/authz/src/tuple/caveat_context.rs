//! [`CaveatContext`]: named values a caveat predicate is evaluated against.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::schema::CaveatValue;

/// A set of named values bound into a caveat evaluation.
///
/// A caveated tuple's stored arguments. The request context of a check has the same shape
/// and must be empty, so a caller cannot widen a grant. Ordered, since it is persisted and
/// hashed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CaveatContext(BTreeMap<String, CaveatValue>);

impl CaveatContext {
    /// The parameter the engine binds to the evaluation's own instant, as a
    /// [`Timestamp`](CaveatValue::Timestamp) in whole seconds since the Unix epoch.
    pub const NOW: &'static str = "now";

    /// An empty context.
    #[must_use]
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Inserts a named value, returning `self` for chaining.
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, value: CaveatValue) -> Self {
        self.0.insert(name.into(), value);
        self
    }

    /// Looks up a named value.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&CaveatValue> {
        self.0.get(name)
    }

    /// Whether the context is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many values the context binds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The first bound name, in name order.
    #[must_use]
    pub fn first_name(&self) -> Option<&str> {
        self.0.keys().next().map(String::as_str)
    }

    /// This context with [`NOW`](Self::NOW) bound to `unix_seconds`, and whether a stored value
    /// of that name was replaced.
    #[must_use]
    pub fn at(&self, unix_seconds: i64) -> (CaveatContext, bool) {
        let mut bound = self.0.clone();
        let replaced = bound
            .insert(Self::NOW.to_owned(), CaveatValue::Timestamp(unix_seconds))
            .is_some();
        (CaveatContext(bound), replaced)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_now_keeps_every_stored_argument() {
        let stored = CaveatContext::new().with("until", CaveatValue::Timestamp(20));
        let (bound, replaced) = stored.at(10);
        assert!(!replaced);
        assert_eq!(bound.get("until"), Some(&CaveatValue::Timestamp(20)));
        assert_eq!(bound.get("now"), Some(&CaveatValue::Timestamp(10)));
    }

    #[test]
    fn binding_now_replaces_a_stored_now_and_says_so() {
        let stored = CaveatContext::new().with("now", CaveatValue::Timestamp(1));
        let (bound, replaced) = stored.at(10);
        assert!(replaced);
        assert_eq!(bound.get("now"), Some(&CaveatValue::Timestamp(10)));
    }

    #[test]
    fn the_first_name_is_in_name_order() {
        let context = CaveatContext::new()
            .with("until", CaveatValue::Int(1))
            .with("now", CaveatValue::Int(2));
        assert_eq!(context.first_name(), Some("now"));
        assert_eq!(context.len(), 2);
        assert_eq!(CaveatContext::new().first_name(), None);
    }

    #[test]
    fn empty_by_default() {
        assert!(CaveatContext::new().is_empty());
    }
}
