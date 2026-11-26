use std::cmp::Ordering;

use crate::Held;
use crate::cells::column;
use crate::compare::compared;
use crate::kinds::{Kind, as_compared, refuse_unpatterned, refuse_unrelated};
use fundamental_db::{DatabaseError, DatabaseResult, Filter, FilterCondition, FilterValue};

/// Refuses `filters` as the database refuses the statement they make, whatever rows it would read.
///
/// # Errors
///
/// [`DatabaseError::Query`] saying what the database would refuse.
pub(crate) fn checked<M: Held>(kinds: &[Kind], filters: &[Filter]) -> DatabaseResult<()> {
    filters
        .iter()
        .try_for_each(|filter| check::<M>(kinds, &filter.condition))
}

fn check<M: Held>(kinds: &[Kind], condition: &FilterCondition) -> DatabaseResult<()> {
    use FilterCondition as C;
    let kind = |column: &str| -> DatabaseResult<Kind> {
        let at = self::column::<M>(column)?;
        Ok(kinds.get(at).copied().unwrap_or(Kind::Other))
    };
    let related = |column: &str, operator: &str, value: &FilterValue| {
        refuse_unrelated(column, kind(column)?, operator, value)
    };
    match condition {
        C::Eq { column, value } => related(column, "=", value),
        C::Ne { column, value } => related(column, "!=", value),
        C::Gt { column, value } => related(column, ">", value),
        C::Gte { column, value } => related(column, ">=", value),
        C::Lt { column, value } => related(column, "<", value),
        C::Lte { column, value } => related(column, "<=", value),
        C::Between { column, low, high } => {
            related(column, ">=", low)?;
            related(column, "<=", high)
        }
        C::In { column, values } | C::NotIn { column, values } => values
            .iter()
            .try_for_each(|value| related(column, "=", value)),
        C::IsNull { column } | C::IsNotNull { column } => kind(column).map(|_| ()),
        C::Like { column, .. } | C::ILike { column, .. } => {
            refuse_unpatterned(column, kind(column)?)
        }
        C::RowValueGt { columns, values } | C::RowValueLt { columns, values } => columns
            .iter()
            .zip(values)
            .try_for_each(|(column, value)| related(column, "<", value)),
        C::And(filters) | C::Or(filters) => filters
            .iter()
            .try_for_each(|filter| check::<M>(kinds, &filter.condition)),
        C::Not(filter) => check::<M>(kinds, &filter.condition),
    }
}

/// Whether the row of `cells` is kept by every one of `filters`.
///
/// # Errors
///
/// [`DatabaseError::Query`] for a column `M`'s table does not have.
pub(crate) fn kept<M: Held>(
    kinds: &[Kind],
    filters: &[Filter],
    cells: &[FilterValue],
) -> DatabaseResult<bool> {
    for filter in filters {
        if verdict::<M>(kinds, filter, cells)? != Some(true) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// What `filter` says of the row: true, false, or `None` as SQL's NULL.
fn verdict<M: Held>(
    kinds: &[Kind],
    filter: &Filter,
    cells: &[FilterValue],
) -> DatabaseResult<Option<bool>> {
    use FilterCondition as C;
    let beside = |column: &str, value: &FilterValue| -> DatabaseResult<FilterValue> {
        let at = self::column::<M>(column)?;
        let kind = kinds.get(at).copied().unwrap_or(Kind::Other);
        Ok(as_compared(kind, cell::<M>(cells, column)?, value))
    };
    let ordered = |column: &str, value: &FilterValue, keep: fn(Ordering) -> bool| {
        Ok(compared(&beside(column, value)?, value).map(keep))
    };
    match &filter.condition {
        C::Eq { column, value } => ordered(column, value, Ordering::is_eq),
        C::Ne { column, value } => ordered(column, value, Ordering::is_ne),
        C::Gt { column, value } => ordered(column, value, Ordering::is_gt),
        C::Gte { column, value } => ordered(column, value, Ordering::is_ge),
        C::Lt { column, value } => ordered(column, value, Ordering::is_lt),
        C::Lte { column, value } => ordered(column, value, Ordering::is_le),
        C::Between { column, low, high } => Ok(and(
            compared(&beside(column, low)?, low).map(Ordering::is_ge),
            compared(&beside(column, high)?, high).map(Ordering::is_le),
        )),
        C::In { column, values } => Ok(within(cell::<M>(cells, column)?, values)),
        C::NotIn { column, values } => {
            Ok(within(cell::<M>(cells, column)?, values).map(|found| !found))
        }
        C::IsNull { column } => Ok(Some(matches!(cell::<M>(cells, column)?, FilterValue::Null))),
        C::IsNotNull { column } => Ok(Some(!matches!(
            cell::<M>(cells, column)?,
            FilterValue::Null
        ))),
        C::Like { column, pattern } => {
            Ok(text(cell::<M>(cells, column)?).map(|held| like(held, pattern)))
        }
        C::ILike { column, pattern } => Ok(text(cell::<M>(cells, column)?)
            .map(|held| like(&held.to_lowercase(), &pattern.to_lowercase()))),
        C::RowValueGt { columns, values } => {
            row_value::<M>(cells, columns, values, Ordering::Greater)
        }
        C::RowValueLt { columns, values } => row_value::<M>(cells, columns, values, Ordering::Less),
        C::And(filters) => {
            let mut all = Some(true);
            for filter in filters {
                all = and(all, verdict::<M>(kinds, filter, cells)?);
            }
            Ok(all)
        }
        C::Or(filters) => {
            let mut any = Some(false);
            for filter in filters {
                any = or(any, verdict::<M>(kinds, filter, cells)?);
            }
            Ok(any)
        }
        C::Not(filter) => Ok(verdict::<M>(kinds, filter, cells)?.map(|kept| !kept)),
    }
}

/// SQL's row comparison `(columns…) > (values…)`, or `<` by `wanted`.
fn row_value<M: Held>(
    cells: &[FilterValue],
    columns: &[String],
    values: &[FilterValue],
    wanted: Ordering,
) -> DatabaseResult<Option<bool>> {
    if columns.len() != values.len() || columns.is_empty() {
        return Err(DatabaseError::query(
            "a row comparison names as many values as columns",
        ));
    }
    for (column, value) in columns.iter().zip(values) {
        match compared(cell::<M>(cells, column)?, value) {
            None => return Ok(None),
            Some(Ordering::Equal) => {}
            Some(order) => return Ok(Some(order == wanted)),
        }
    }
    Ok(Some(false))
}

/// SQL's `IN`, with no answer for a NULL `held` or a list holding a NULL and not `held`.
fn within(held: &FilterValue, values: &[FilterValue]) -> Option<bool> {
    if matches!(held, FilterValue::Null) {
        return None;
    }
    if values
        .iter()
        .any(|value| compared(held, value) == Some(Ordering::Equal))
    {
        return Some(true);
    }
    if values
        .iter()
        .any(|value| matches!(value, FilterValue::Null))
    {
        return None;
    }
    Some(false)
}

fn text(held: &FilterValue) -> Option<&str> {
    match held {
        FilterValue::String(text) | FilterValue::Uuid(text) => Some(text),
        _ => None,
    }
}

/// Whether `text` matches the `LIKE` `pattern`, with `\` as the escape.
fn like(text: &str, pattern: &str) -> bool {
    let text: Vec<char> = text.chars().collect();
    let pattern = pieces(pattern);
    // `reach[i]`: whether the pattern read so far matches the first `i` characters.
    let mut reach = vec![false; text.len() + 1];
    reach[0] = true;
    for piece in pattern {
        reach = match piece {
            Piece::Any => {
                let mut next = reach.clone();
                for at in 1..next.len() {
                    next[at] = next[at] || next[at - 1];
                }
                next
            }
            Piece::One | Piece::Char(_) => {
                let mut next = vec![false; text.len() + 1];
                for (at, character) in text.iter().enumerate() {
                    let fits = match piece {
                        Piece::Char(wanted) => *character == wanted,
                        _ => true,
                    };
                    next[at + 1] = reach[at] && fits;
                }
                next
            }
        };
    }
    reach[text.len()]
}

#[derive(Clone, Copy)]
enum Piece {
    /// `%`
    Any,
    /// `_`
    One,
    Char(char),
}

fn pieces(pattern: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut characters = pattern.chars();
    while let Some(character) = characters.next() {
        pieces.push(match character {
            '%' => Piece::Any,
            '_' => Piece::One,
            '\\' => Piece::Char(characters.next().unwrap_or('\\')),
            other => Piece::Char(other),
        });
    }
    pieces
}

const fn or(left: Option<bool>, right: Option<bool>) -> Option<bool> {
    match (left, right) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
        _ => None,
    }
}

const fn and(left: Option<bool>, right: Option<bool>) -> Option<bool> {
    match (left, right) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => Some(true),
        _ => None,
    }
}

fn cell<'a, M: Held>(cells: &'a [FilterValue], column: &str) -> DatabaseResult<&'a FilterValue> {
    let at = self::column::<M>(column)?;
    cells
        .get(at)
        .ok_or_else(|| DatabaseError::query(format!("column \"{column}\" is not held")))
}
