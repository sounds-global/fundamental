use crate::SqlError;
use crate::ident::validate_identifier;
use fundamental_db::{FilterCondition, FilterValue};

/// Builds a filter condition SQL fragment, refusing an unsafe identifier; values are returned for `$N` binding.
#[expect(clippy::too_many_lines, reason = "readability baseline")]
pub(crate) fn build_filter_condition(
    condition: &FilterCondition,
    start_index: usize,
) -> Result<(String, Vec<FilterValue>, usize), SqlError> {
    let mut index = start_index;

    let result = match condition {
        FilterCondition::Eq { column, value } => {
            validate_identifier(column, false)?;
            let clause = format!("{} = ${}", column, index);
            index += 1;
            (clause, vec![value.clone()], index)
        }
        FilterCondition::Ne { column, value } => {
            validate_identifier(column, false)?;
            let clause = format!("{} != ${}", column, index);
            index += 1;
            (clause, vec![value.clone()], index)
        }
        FilterCondition::Gt { column, value } => {
            validate_identifier(column, false)?;
            let clause = format!("{} > ${}", column, index);
            index += 1;
            (clause, vec![value.clone()], index)
        }
        FilterCondition::Gte { column, value } => {
            validate_identifier(column, false)?;
            let clause = format!("{} >= ${}", column, index);
            index += 1;
            (clause, vec![value.clone()], index)
        }
        FilterCondition::Lt { column, value } => {
            validate_identifier(column, false)?;
            let clause = format!("{} < ${}", column, index);
            index += 1;
            (clause, vec![value.clone()], index)
        }
        FilterCondition::Lte { column, value } => {
            validate_identifier(column, false)?;
            let clause = format!("{} <= ${}", column, index);
            index += 1;
            (clause, vec![value.clone()], index)
        }
        FilterCondition::In { column, values } => {
            validate_identifier(column, false)?;
            if values.is_empty() {
                // An empty IN list is always false.
                ("1 = 0".to_string(), vec![], index)
            } else {
                let placeholders: Vec<String> = values
                    .iter()
                    .map(|_| {
                        let p = format!("${}", index);
                        index += 1;
                        p
                    })
                    .collect();
                let clause = format!("{} IN ({})", column, placeholders.join(", "));
                (clause, values.clone(), index)
            }
        }
        FilterCondition::NotIn { column, values } => {
            validate_identifier(column, false)?;
            if values.is_empty() {
                // An empty NOT IN list is always true.
                ("1 = 1".to_string(), vec![], index)
            } else {
                let placeholders: Vec<String> = values
                    .iter()
                    .map(|_| {
                        let p = format!("${}", index);
                        index += 1;
                        p
                    })
                    .collect();
                let clause = format!("{} NOT IN ({})", column, placeholders.join(", "));
                (clause, values.clone(), index)
            }
        }
        FilterCondition::IsNull { column } => {
            validate_identifier(column, false)?;
            let clause = format!("{} IS NULL", column);
            (clause, vec![], index)
        }
        FilterCondition::IsNotNull { column } => {
            validate_identifier(column, false)?;
            let clause = format!("{} IS NOT NULL", column);
            (clause, vec![], index)
        }
        FilterCondition::Like { column, pattern } => {
            validate_identifier(column, false)?;
            let clause = format!("{} LIKE ${}", column, index);
            index += 1;
            (clause, vec![FilterValue::String(pattern.clone())], index)
        }
        FilterCondition::ILike { column, pattern } => {
            validate_identifier(column, false)?;
            let clause = format!("{} ILIKE ${}", column, index);
            index += 1;
            (clause, vec![FilterValue::String(pattern.clone())], index)
        }
        FilterCondition::Between { column, low, high } => {
            validate_identifier(column, false)?;
            let clause = format!("{} BETWEEN ${} AND ${}", column, index, index + 1);
            index += 2;
            (clause, vec![low.clone(), high.clone()], index)
        }
        FilterCondition::RowValueGt { columns, values } => {
            row_value_clause(columns, values, ">", &mut index)?
        }
        FilterCondition::RowValueLt { columns, values } => {
            row_value_clause(columns, values, "<", &mut index)?
        }
        FilterCondition::And(filters) => {
            let mut clauses = Vec::new();
            let mut all_values = Vec::new();
            for filter in filters {
                let (clause, values, next_index) =
                    build_filter_condition(&filter.condition, index)?;
                clauses.push(clause);
                all_values.extend(values);
                index = next_index;
            }
            let clause = format!("({})", clauses.join(" AND "));
            (clause, all_values, index)
        }
        FilterCondition::Or(filters) => {
            let mut clauses = Vec::new();
            let mut all_values = Vec::new();
            for filter in filters {
                let (clause, values, next_index) =
                    build_filter_condition(&filter.condition, index)?;
                clauses.push(clause);
                all_values.extend(values);
                index = next_index;
            }
            let clause = format!("({})", clauses.join(" OR "));
            (clause, all_values, index)
        }
        FilterCondition::Not(filter) => {
            let (inner_clause, values, next_index) =
                build_filter_condition(&filter.condition, index)?;
            let clause = format!("NOT ({})", inner_clause);
            (clause, values, next_index)
        }
    };

    Ok(result)
}

/// Builds a row-value comparison, `(a, b) > ($1, $2)`, for keyset pagination, at the columns' own types.
///
/// # Errors
/// [`SqlError::query`] if the tuple is empty or the two sides differ in length.
fn row_value_clause(
    columns: &[String],
    values: &[FilterValue],
    operator: &str,
    index: &mut usize,
) -> Result<(String, Vec<FilterValue>, usize), SqlError> {
    if columns.is_empty() {
        return Err(SqlError::query(
            "a row-value comparison needs at least one column",
        ));
    }
    if columns.len() != values.len() {
        return Err(SqlError::query(format!(
            "a row-value comparison needs one value per column; got {} columns and {} values",
            columns.len(),
            values.len()
        )));
    }
    for column in columns {
        validate_identifier(column, false)?;
    }
    let placeholders: Vec<String> = (0..values.len())
        .map(|offset| format!("${}", *index + offset))
        .collect();
    let clause = format!(
        "({}) {operator} ({})",
        columns.join(", "),
        placeholders.join(", ")
    );
    *index += values.len();
    Ok((clause, values.to_vec(), *index))
}
