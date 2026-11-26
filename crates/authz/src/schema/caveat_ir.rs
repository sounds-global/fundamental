//! [`CaveatIr`]: a bounded boolean predicate over named parameters.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::CaveatValue;

/// A comparison operator used inside a [`CaveatIr::Compare`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompareOp {
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Lte,
    /// `>`
    Gt,
    /// `>=`
    Gte,
}

/// One side of a comparison: a named parameter, or a literal value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CaveatOperand {
    /// A named parameter, resolved from the tuple's stored caveat arguments at evaluation time.
    Param(String),
    /// A literal value baked into the schema.
    Literal(CaveatValue),
}

/// A caveat predicate: a closed, side-effect-free boolean expression over named parameters,
/// which makes a relationship edge conditional without running arbitrary code.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CaveatIr {
    /// A constant.
    Bool(bool),
    /// A boolean-valued named parameter.
    Param(String),
    /// Logical conjunction — true when every child is true (empty ⇒ true).
    And(Vec<CaveatIr>),
    /// Logical disjunction — true when any child is true (empty ⇒ false).
    Or(Vec<CaveatIr>),
    /// Logical negation.
    Not(Box<CaveatIr>),
    /// A comparison between two operands.
    Compare {
        /// The left-hand operand.
        left: CaveatOperand,
        /// The comparison operator.
        op: CompareOp,
        /// The right-hand operand.
        right: CaveatOperand,
    },
}

impl CaveatIr {
    /// Collects the names of every parameter this predicate reads.
    ///
    /// Used to validate that every referenced parameter is declared.
    #[must_use]
    pub fn referenced_params(&self) -> BTreeSet<String> {
        let mut params = BTreeSet::new();
        self.collect_params(&mut params);
        params
    }

    fn collect_params(&self, out: &mut BTreeSet<String>) {
        match self {
            CaveatIr::Bool(_) => {}
            CaveatIr::Param(name) => {
                out.insert(name.clone());
            }
            CaveatIr::And(children) | CaveatIr::Or(children) => {
                for child in children {
                    child.collect_params(out);
                }
            }
            CaveatIr::Not(inner) => inner.collect_params(out),
            CaveatIr::Compare { left, right, .. } => {
                if let CaveatOperand::Param(name) = left {
                    out.insert(name.clone());
                }
                if let CaveatOperand::Param(name) = right {
                    out.insert(name.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_referenced_params_across_the_tree() {
        let ir = CaveatIr::And(vec![
            CaveatIr::Compare {
                left: CaveatOperand::Param("now".into()),
                op: CompareOp::Lt,
                right: CaveatOperand::Param("valid_until".into()),
            },
            CaveatIr::Param("active".into()),
        ]);
        let params = ir.referenced_params();
        assert!(params.contains("now"));
        assert!(params.contains("valid_until"));
        assert!(params.contains("active"));
        assert_eq!(params.len(), 3);
    }

    #[test]
    fn literals_contribute_no_params() {
        let ir = CaveatIr::Compare {
            left: CaveatOperand::Param("region".into()),
            op: CompareOp::Eq,
            right: CaveatOperand::Literal(CaveatValue::String("eu".into())),
        };
        assert_eq!(
            ir.referenced_params(),
            BTreeSet::from(["region".to_string()])
        );
    }
}
