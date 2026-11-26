//! [`DirectEdge`]: the result of a point lookup for one stored edge.

use super::Caveat;

/// What a point lookup found for a specific `object#relation@subject` edge.
#[derive(Clone, Debug, PartialEq)]
pub enum DirectEdge {
    /// The edge exists and grants unconditionally.
    Unconditional,
    /// The edge exists but grants only if this caveat evaluates true.
    Conditional(Caveat),
}

impl DirectEdge {
    /// Builds the appropriate variant from an optional stored caveat.
    #[must_use]
    pub fn from_caveat(caveat: Option<Caveat>) -> Self {
        match caveat {
            Some(caveat) => DirectEdge::Conditional(caveat),
            None => DirectEdge::Unconditional,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::CaveatName;
    use crate::tuple::CaveatContext;

    #[test]
    fn maps_optional_caveat() {
        assert_eq!(DirectEdge::from_caveat(None), DirectEdge::Unconditional);
        let c = Caveat::new(CaveatName::new("c").expect("valid"), CaveatContext::new());
        assert_eq!(
            DirectEdge::from_caveat(Some(c.clone())),
            DirectEdge::Conditional(c)
        );
    }
}
