//! [`Caveat`]: the concrete caveat attached to a single tuple edge.

use serde::{Deserialize, Serialize};

use super::CaveatContext;
use crate::schema::CaveatName;

/// A caveat instance stored on a relation tuple: which predicate gates the edge,
/// and the arguments baked into this particular edge.
///
/// Evaluated against these arguments with the evaluation's own [`NOW`](CaveatContext::NOW),
/// never against anything the caller supplied.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Caveat {
    /// The caveat predicate to evaluate (defined on the object's namespace).
    pub name: CaveatName,
    /// The arguments baked into this edge.
    #[serde(default, skip_serializing_if = "CaveatContext::is_empty")]
    pub context: CaveatContext,
}

impl Caveat {
    /// Builds a caveat instance.
    #[must_use]
    pub fn new(name: CaveatName, context: CaveatContext) -> Self {
        Self { name, context }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::CaveatValue;

    #[test]
    fn carries_name_and_context() {
        let c = Caveat::new(
            CaveatName::new("not_expired").expect("valid"),
            CaveatContext::new().with("valid_until", CaveatValue::Timestamp(100)),
        );
        assert_eq!(c.name.as_str(), "not_expired");
        assert_eq!(
            c.context.get("valid_until"),
            Some(&CaveatValue::Timestamp(100))
        );
    }
}
