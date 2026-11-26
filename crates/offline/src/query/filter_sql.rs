//! Rendering the framework's abstract [`Filter`] tree into parameterized SQLite
//!
//! Values are bound as `?N` placeholders; `ILIKE` maps to `LIKE`, which SQLite already
//! treats as ASCII-case-insensitive.

use fundamental_db::ident::validate_identifier;
use fundamental_db::{
    DatabaseError, DatabaseResult, Filter, FilterCondition, FilterValue, OrderBy, OrderDirection,
    OrderNulls,
};

use crate::db::Value;

/// Converts an abstract [`FilterValue`] into an offline [`Value`] for binding.
pub(crate) fn to_value(value: &FilterValue) -> Value {
    match value {
        FilterValue::Null => Value::Null,
        FilterValue::Bool(b) => Value::Integer(i64::from(*b)),
        FilterValue::Int(i) => Value::Integer(*i),
        FilterValue::Timestamp(ms) => Value::Integer(*ms),
        FilterValue::Float(f) => Value::Real(*f),
        FilterValue::String(s) | FilterValue::Uuid(s) => Value::Text(s.clone()),
        FilterValue::Bytes(b) => Value::Blob(b.clone()),
        FilterValue::TimestampTz(dt) => Value::Text(dt.to_rfc3339()),
        FilterValue::Json(v) => Value::Text(v.to_string()),
    }
}

/// Validates a column identifier, mapping a rejection to a query error.
fn column(name: &str) -> DatabaseResult<&str> {
    validate_identifier(name, false)
        .map_err(|e| DatabaseError::query(format!("rejected unsafe identifier: {e}")))?;
    Ok(name)
}

/// Renders the `AND`-joined bodies of `filters`, binding values into `params` from `?next`.
pub(crate) fn render_filters(
    filters: &[Filter],
    next: &mut usize,
    params: &mut Vec<Value>,
) -> DatabaseResult<String> {
    let mut clauses = Vec::with_capacity(filters.len());
    for filter in filters {
        clauses.push(render_condition(&filter.condition, next, params)?);
    }
    Ok(clauses.join(" AND "))
}

#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn render_condition(
    condition: &FilterCondition,
    next: &mut usize,
    params: &mut Vec<Value>,
) -> DatabaseResult<String> {
    let mut placeholder = |params: &mut Vec<Value>, value: Value| {
        params.push(value);
        let p = format!("?{next}");
        *next += 1;
        p
    };

    let clause = match condition {
        FilterCondition::Eq { column: c, value } => {
            format!("{} = {}", column(c)?, placeholder(params, to_value(value)))
        }
        FilterCondition::Ne { column: c, value } => {
            format!("{} != {}", column(c)?, placeholder(params, to_value(value)))
        }
        FilterCondition::Gt { column: c, value } => {
            format!("{} > {}", column(c)?, placeholder(params, to_value(value)))
        }
        FilterCondition::Gte { column: c, value } => {
            format!("{} >= {}", column(c)?, placeholder(params, to_value(value)))
        }
        FilterCondition::Lt { column: c, value } => {
            format!("{} < {}", column(c)?, placeholder(params, to_value(value)))
        }
        FilterCondition::Lte { column: c, value } => {
            format!("{} <= {}", column(c)?, placeholder(params, to_value(value)))
        }
        FilterCondition::In { column: c, values } => {
            let c = column(c)?;
            if values.is_empty() {
                "1 = 0".to_string() // empty IN is always false
            } else {
                let placeholders: Vec<String> = values
                    .iter()
                    .map(|v| placeholder(params, to_value(v)))
                    .collect();
                format!("{c} IN ({})", placeholders.join(", "))
            }
        }
        FilterCondition::NotIn { column: c, values } => {
            let c = column(c)?;
            if values.is_empty() {
                "1 = 1".to_string() // empty NOT IN is always true
            } else {
                let placeholders: Vec<String> = values
                    .iter()
                    .map(|v| placeholder(params, to_value(v)))
                    .collect();
                format!("{c} NOT IN ({})", placeholders.join(", "))
            }
        }
        FilterCondition::IsNull { column: c } => format!("{} IS NULL", column(c)?),
        FilterCondition::IsNotNull { column: c } => format!("{} IS NOT NULL", column(c)?),
        FilterCondition::Like { column: c, pattern }
        | FilterCondition::ILike { column: c, pattern } => {
            format!(
                "{} LIKE {}",
                column(c)?,
                placeholder(params, Value::Text(pattern.clone()))
            )
        }
        FilterCondition::Between {
            column: c,
            low,
            high,
        } => {
            let c = column(c)?;
            let low = placeholder(params, to_value(low));
            let high = placeholder(params, to_value(high));
            format!("{c} BETWEEN {low} AND {high}")
        }
        FilterCondition::RowValueGt {
            columns: cols,
            values,
        } => row_value(cols, values, ">", &mut placeholder, params)?,
        FilterCondition::RowValueLt {
            columns: cols,
            values,
        } => row_value(cols, values, "<", &mut placeholder, params)?,
        FilterCondition::And(filters) => {
            format!(
                "({})",
                render_child_conditions(filters, " AND ", next, params)?
            )
        }
        FilterCondition::Or(filters) => {
            format!(
                "({})",
                render_child_conditions(filters, " OR ", next, params)?
            )
        }
        FilterCondition::Not(filter) => {
            format!(
                "NOT ({})",
                render_condition(&filter.condition, next, params)?
            )
        }
    };
    Ok(clause)
}

/// Renders child filters joined by `sep` (for `And`/`Or`).
fn render_child_conditions(
    filters: &[Filter],
    sep: &str,
    next: &mut usize,
    params: &mut Vec<Value>,
) -> DatabaseResult<String> {
    let mut clauses = Vec::with_capacity(filters.len());
    for filter in filters {
        clauses.push(render_condition(&filter.condition, next, params)?);
    }
    Ok(clauses.join(sep))
}

/// Renders an `ORDER BY … LIMIT … OFFSET …` tail (empty string for none).
pub(crate) fn render_order_limit(
    order_by: &[OrderBy],
    limit: Option<u64>,
    offset: Option<u64>,
) -> DatabaseResult<String> {
    let mut parts: Vec<String> = Vec::new();
    if !order_by.is_empty() {
        let mut terms = Vec::with_capacity(order_by.len());
        for order in order_by {
            let direction = match order.direction {
                OrderDirection::Asc => "ASC",
                OrderDirection::Desc => "DESC",
            };
            let nulls = match order.nulls {
                Some(OrderNulls::First) => " NULLS FIRST",
                Some(OrderNulls::Last) => " NULLS LAST",
                None => "",
            };
            terms.push(format!("{} {direction}{nulls}", column(&order.column)?));
        }
        parts.push(format!("ORDER BY {}", terms.join(", ")));
    }
    if let Some(limit) = limit {
        parts.push(format!("LIMIT {limit}"));
        if let Some(offset) = offset {
            parts.push(format!("OFFSET {offset}"));
        }
    }
    Ok(parts.join(" "))
}

/// Renders a `SET col = ?N, …` assignment list for an update.
pub(crate) fn render_set(
    updates: &[(String, FilterValue)],
    next: &mut usize,
    params: &mut Vec<Value>,
) -> DatabaseResult<String> {
    let mut assignments = Vec::with_capacity(updates.len());
    for (col, value) in updates {
        params.push(to_value(value));
        assignments.push(format!("{} = ?{next}", column(col)?));
        *next += 1;
    }
    Ok(assignments.join(", "))
}

/// Renders a row-value comparison `(a, b) > (?1, ?2)`, the keyset-pagination predicate.
///
/// # Errors
/// [`DatabaseError::query`] if the tuple is empty or the sides differ in length; a prefix
/// comparison would silently skip rows.
fn row_value(
    columns: &[String],
    values: &[FilterValue],
    operator: &str,
    placeholder: &mut impl FnMut(&mut Vec<Value>, Value) -> String,
    params: &mut Vec<Value>,
) -> DatabaseResult<String> {
    if columns.is_empty() {
        return Err(DatabaseError::query(
            "a row-value comparison needs at least one column",
        ));
    }
    if columns.len() != values.len() {
        return Err(DatabaseError::query(format!(
            "a row-value comparison needs one value per column; got {} columns and {} values",
            columns.len(),
            values.len()
        )));
    }
    let mut names = Vec::with_capacity(columns.len());
    for name in columns {
        names.push(column(name)?);
    }
    let bound: Vec<String> = values
        .iter()
        .map(|value| placeholder(params, to_value(value)))
        .collect();
    Ok(format!(
        "({}) {operator} ({})",
        names.join(", "),
        bound.join(", ")
    ))
}
