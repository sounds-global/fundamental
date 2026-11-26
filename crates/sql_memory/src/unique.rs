use fundamental_db::{DatabaseError, DatabaseResult, Filter, FilterValue};

use crate::Held;
use crate::cells::column;
use crate::compare::same;
use crate::kinds::{Kind, kinds};
use crate::matching::{checked, kept};

pub(crate) struct Unique {
    name: String,
    columns: Vec<usize>,
    holds: Vec<Filter>,
    kinds: Vec<Kind>,
}

/// The unique indexes `M`'s table declares.
///
/// # Errors
///
/// [`DatabaseError::Configuration`] for an index whose condition cannot be read.
pub(crate) fn unique_indexes<M: Held>() -> DatabaseResult<Vec<Unique>> {
    let kinds = kinds::<M>();
    M::table_schema()
        .indexes
        .into_values()
        .filter(|index| index.unique)
        .map(|index| {
            let columns = index
                .columns
                .iter()
                .map(|name| column::<M>(name))
                .collect::<DatabaseResult<Vec<_>>>()?;
            let holds = match index.condition.as_deref() {
                Some(condition) => read_condition(condition).ok_or_else(|| {
                    DatabaseError::configuration(format!(
                        "the memory database cannot read `{condition}`, the condition of the \
                         unique index `{}`: it reads `<column> IS [NOT] NULL`, `<column> = \
                         <value>` and `<column> IN (<values>)` joined by AND",
                        index.name
                    ))
                })?,
                None => Vec::new(),
            };
            checked::<M>(&kinds, &holds).map_err(|error| {
                DatabaseError::configuration(format!(
                    "the condition of the unique index `{}`: {error}",
                    index.name
                ))
            })?;
            Ok(Unique {
                name: index.name,
                columns,
                holds,
                kinds: kinds.clone(),
            })
        })
        .collect()
}

/// Refuses `row` as a second row of one of `indexes` among `others`.
///
/// # Errors
///
/// [`DatabaseError::ConstraintViolation`] naming the index `row` clashes on.
pub(crate) fn refuse_clash<'a, M: Held>(
    indexes: &[Unique],
    row: &[FilterValue],
    others: impl Iterator<Item = &'a [FilterValue]> + Clone,
) -> DatabaseResult<()> {
    for index in indexes {
        if !index.holds::<M>(row)? {
            continue;
        }
        for other in others.clone() {
            if index.holds::<M>(other)? && index.same(row, other) {
                return Err(DatabaseError::unique_violation(index.name.clone()));
            }
        }
    }
    Ok(())
}

impl Unique {
    /// Whether the index holds the row: its condition keeps it and none of its columns is NULL.
    fn holds<M: Held>(&self, cells: &[FilterValue]) -> DatabaseResult<bool> {
        let null = self
            .columns
            .iter()
            .any(|at| matches!(cells.get(*at), None | Some(FilterValue::Null)));
        Ok(!null && kept::<M>(&self.kinds, &self.holds, cells)?)
    }

    fn same(&self, left: &[FilterValue], right: &[FilterValue]) -> bool {
        self.columns
            .iter()
            .all(|at| matches!((left.get(*at), right.get(*at)), (Some(a), Some(b)) if same(a, b)))
    }
}

/// A partial index's `condition` as AND-joined filters, or `None` when it says more.
fn read_condition(condition: &str) -> Option<Vec<Filter>> {
    and_parts(condition).into_iter().map(read_part).collect()
}

fn and_parts(condition: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let bytes = condition.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'\'' {
            quoted = !quoted;
        } else if !quoted
            && condition
                .get(at..at + 5)
                .is_some_and(|word| word.eq_ignore_ascii_case(" and "))
        {
            parts.push(condition[start..at].trim());
            start = at + 5;
            at += 4;
        }
        at += 1;
    }
    parts.push(condition[start..].trim());
    parts
}

fn read_part(part: &str) -> Option<Filter> {
    let part = part.trim();
    let (column, rest) = part.split_once(char::is_whitespace)?;
    if !column
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    let rest = rest.trim();
    let upper = rest.to_ascii_uppercase();
    if upper == "IS NULL" {
        return Some(Filter::is_null(column));
    }
    if upper == "IS NOT NULL" {
        return Some(Filter::is_not_null(column));
    }
    if let Some(value) = rest.strip_prefix('=') {
        return Some(Filter::eq(column, literal(value)?));
    }
    if upper.starts_with("IN") {
        let list = rest[2..].trim().strip_prefix('(')?.strip_suffix(')')?;
        let values = list
            .split(',')
            .map(literal)
            .collect::<Option<Vec<FilterValue>>>()?;
        return Some(Filter::in_list(column, values));
    }
    None
}

fn literal(text: &str) -> Option<FilterValue> {
    let text = text.trim();
    if let Some(inner) = text.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')) {
        return Some(FilterValue::String(inner.replace("''", "'")));
    }
    text.parse::<i64>().ok().map(FilterValue::Int)
}

#[cfg(test)]
mod tests {
    use super::read_condition;

    #[test]
    fn a_condition_reads_as_the_filters_it_joins() {
        for condition in [
            "deleted_at IS NULL",
            "closed_at IS NULL AND outbox_set_aside_at IS NULL AND outbox_skip_until IS NOT NULL",
            "state = 'clean' AND claim_by IS NOT NULL",
            "state IN ('requested', 'queued', 'pending')",
            "state = 1",
        ] {
            assert!(read_condition(condition).is_some(), "{condition}");
        }
    }

    #[test]
    fn a_condition_saying_more_is_not_read() {
        for condition in ["lower(slug) <> ''", "a IS NULL OR b IS NULL", "rank > 2"] {
            assert!(read_condition(condition).is_none(), "{condition}");
        }
    }
}
