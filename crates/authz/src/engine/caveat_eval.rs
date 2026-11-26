//! Evaluation of a [`CaveatIr`] predicate against a [`CaveatContext`]; a missing parameter or
//! an incomparable comparison is an error, never a silent `false`.

use std::cmp::Ordering;

use crate::error::AuthzError;
use crate::schema::{CaveatIr, CaveatOperand, CaveatValue, CompareOp};
use crate::tuple::CaveatContext;

/// Evaluates `caveat` against `context`.
///
/// # Errors
///
/// Returns [`AuthzError::CaveatEval`] if a referenced parameter is absent, a
/// parameter used as a boolean is not one, or a `<`/`>` comparison is applied to
/// incomparable value types.
pub(crate) fn evaluate(caveat: &CaveatIr, context: &CaveatContext) -> Result<bool, AuthzError> {
    match caveat {
        CaveatIr::Bool(value) => Ok(*value),
        CaveatIr::Param(name) => match context.get(name) {
            Some(CaveatValue::Bool(value)) => Ok(*value),
            Some(other) => Err(AuthzError::CaveatEval(format!(
                "parameter `{name}` is used as a boolean but is a {}",
                other.type_name()
            ))),
            None => Err(AuthzError::CaveatEval(format!(
                "missing parameter `{name}`"
            ))),
        },
        CaveatIr::And(children) => {
            for child in children {
                if !evaluate(child, context)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        CaveatIr::Or(children) => {
            for child in children {
                if evaluate(child, context)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        CaveatIr::Not(inner) => Ok(!evaluate(inner, context)?),
        CaveatIr::Compare { left, op, right } => {
            let left = resolve(left, context)?;
            let right = resolve(right, context)?;
            compare(&left, *op, &right)
        }
    }
}

fn resolve(operand: &CaveatOperand, context: &CaveatContext) -> Result<CaveatValue, AuthzError> {
    match operand {
        CaveatOperand::Literal(value) => Ok(value.clone()),
        CaveatOperand::Param(name) => context
            .get(name)
            .cloned()
            .ok_or_else(|| AuthzError::CaveatEval(format!("missing parameter `{name}`"))),
    }
}

fn compare(left: &CaveatValue, op: CompareOp, right: &CaveatValue) -> Result<bool, AuthzError> {
    match op {
        CompareOp::Eq => Ok(left == right),
        CompareOp::Ne => Ok(left != right),
        CompareOp::Lt => Ok(ordering(left, right)? == Ordering::Less),
        CompareOp::Lte => Ok(ordering(left, right)? != Ordering::Greater),
        CompareOp::Gt => Ok(ordering(left, right)? == Ordering::Greater),
        CompareOp::Gte => Ok(ordering(left, right)? != Ordering::Less),
    }
}

/// Orders two comparable values: ints and timestamps as integers, an int and a double as
/// reals, strings lexically; any other pairing is an error.
fn ordering(left: &CaveatValue, right: &CaveatValue) -> Result<Ordering, AuthzError> {
    use CaveatValue::{Double, Int, String as Str, Timestamp};
    let result = match (left, right) {
        (Int(a), Int(b)) | (Timestamp(a), Timestamp(b)) => a.cmp(b),
        (Double(a), Double(b)) => a
            .partial_cmp(b)
            .ok_or_else(|| AuthzError::CaveatEval("cannot order NaN".to_string()))?,
        (Int(a), Double(b)) => (*a as f64)
            .partial_cmp(b)
            .ok_or_else(|| AuthzError::CaveatEval("cannot order NaN".to_string()))?,
        (Double(a), Int(b)) => a
            .partial_cmp(&(*b as f64))
            .ok_or_else(|| AuthzError::CaveatEval("cannot order NaN".to_string()))?,
        (Str(a), Str(b)) => a.cmp(b),
        (a, b) => {
            return Err(AuthzError::CaveatEval(format!(
                "cannot order a {} against a {}",
                a.type_name(),
                b.type_name()
            )));
        }
    };
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> CaveatContext {
        CaveatContext::new()
            .with("now", CaveatValue::Timestamp(1000))
            .with("valid_until", CaveatValue::Timestamp(2000))
            .with("region", CaveatValue::String("eu".into()))
            .with("active", CaveatValue::Bool(true))
    }

    fn param(name: &str) -> CaveatOperand {
        CaveatOperand::Param(name.into())
    }
    fn lit(value: CaveatValue) -> CaveatOperand {
        CaveatOperand::Literal(value)
    }

    #[test]
    fn evaluates_time_window() {
        let ir = CaveatIr::Compare {
            left: param("now"),
            op: CompareOp::Lt,
            right: param("valid_until"),
        };
        assert!(evaluate(&ir, &ctx()).expect("eval"));
    }

    #[test]
    fn evaluates_boolean_composition() {
        let ir = CaveatIr::And(vec![
            CaveatIr::Param("active".into()),
            CaveatIr::Compare {
                left: param("region"),
                op: CompareOp::Eq,
                right: lit(CaveatValue::String("eu".into())),
            },
        ]);
        assert!(evaluate(&ir, &ctx()).expect("eval"));

        let ir_us = CaveatIr::Compare {
            left: param("region"),
            op: CompareOp::Eq,
            right: lit(CaveatValue::String("us".into())),
        };
        assert!(!evaluate(&ir_us, &ctx()).expect("eval"));
    }

    #[test]
    fn missing_parameter_is_an_error_not_false() {
        let ir = CaveatIr::Param("undeclared".into());
        let err = evaluate(&ir, &ctx()).expect_err("must error");
        assert!(matches!(err, AuthzError::CaveatEval(_)));
    }

    #[test]
    fn incomparable_types_error() {
        let ir = CaveatIr::Compare {
            left: param("active"), // bool
            op: CompareOp::Lt,
            right: lit(CaveatValue::Int(1)),
        };
        assert!(matches!(
            evaluate(&ir, &ctx()).expect_err("must error"),
            AuthzError::CaveatEval(_)
        ));
    }

    #[test]
    fn int_and_double_compare_as_reals() {
        let ir = CaveatIr::Compare {
            left: lit(CaveatValue::Int(3)),
            op: CompareOp::Lt,
            right: lit(CaveatValue::Double(3.5)),
        };
        assert!(evaluate(&ir, &CaveatContext::new()).expect("eval"));
    }
}
