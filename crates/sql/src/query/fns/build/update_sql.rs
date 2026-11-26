//! Builds `UPDATE` statements from a [`QueryBuilder`] and a list of column updates.

use super::build_where_clause;
use crate::SqlError;
use crate::ident::validate_identifier;

use fundamental_core::model::{Model, SqlDb, SqlModel};
use fundamental_core::outbox::OutboxColumns;
use fundamental_db::{FilterValue, QueryBuilder, SchemaName};

/// Builds an UPDATE SQL statement, refusing an unsafe identifier; `NULL` values emit a literal.
pub(crate) fn build_update_sql<M: SqlModel, State>(
    schema: &SchemaName,
    query: &QueryBuilder<M, SqlDb, State>,
    updates: &[(String, FilterValue)],
) -> Result<(String, Vec<FilterValue>), SqlError> {
    refuse_unbinding_updates::<M>(updates)?;
    refuse_unsealing_updates::<M>(updates)?;

    let mut values = Vec::new();
    let mut param_index = 1;

    let mut sql = format!("UPDATE {} SET ", schema.qualify(M::TABLE));

    let mut set_clauses: Vec<String> = Vec::with_capacity(updates.len());
    for (col, val) in updates {
        validate_identifier(col, false)?;
        let clause = match val {
            FilterValue::Null => {
                format!("{} = NULL", col)
            }
            _ => {
                values.push(val.clone());
                let clause = format!("{} = ${}", col, param_index);
                param_index += 1;
                clause
            }
        };
        set_clauses.push(clause);
    }
    sql.push_str(&set_clauses.join(", "));

    if !query.get_filters().is_empty() {
        let (where_clause, where_values, _) = build_where_clause(query.get_filters(), param_index)?;
        if !where_clause.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clause);
        }
        values.extend(where_values);
    }

    Ok((sql, values))
}

/// Refuses a partial update that would write plaintext into an encrypted column or change a bound
/// column; erasure, every encrypted column to `NULL`, is the one sound exception.
fn refuse_unbinding_updates<M: Model>(updates: &[(String, FilterValue)]) -> Result<(), SqlError> {
    let secrets: Vec<&str> = M::secret_columns()
        .iter()
        .map(|(column, _)| *column)
        .collect();
    if secrets.is_empty() {
        return Ok(());
    }
    // Unquoted identifiers fold to lower case, so `VALUE` writes `value`.
    let names = |declared: &str, column: &str| declared.eq_ignore_ascii_case(column);
    for (column, value) in updates {
        if secrets.iter().any(|secret| names(secret, column)) && !matches!(value, FilterValue::Null)
        {
            return Err(SqlError::Config(format!(
                "column '{column}' of {} is encrypted; a partial update may only erase it (NULL) — write the model through its repository to encrypt a value",
                M::TYPE_NAME
            )));
        }
    }
    let bound: Vec<&String> = updates
        .iter()
        .filter(|(column, _)| {
            M::row_bound_columns()
                .iter()
                .any(|bound| names(bound, column))
        })
        .map(|(column, _)| column)
        .collect();
    if bound.is_empty() {
        return Ok(());
    }
    let erases_every_secret = secrets.iter().all(|secret| {
        updates
            .iter()
            .any(|(column, value)| names(secret, column) && matches!(value, FilterValue::Null))
    });
    let clears_bound_only = updates
        .iter()
        .filter(|(column, _)| {
            M::row_bound_columns()
                .iter()
                .any(|bound| names(bound, column))
        })
        .all(|(_, value)| matches!(value, FilterValue::Null));
    if erases_every_secret && clears_bound_only {
        return Ok(());
    }
    Err(SqlError::Config(format!(
        "columns {bound:?} of {} are bound into its encrypted values; a partial update may change them only by clearing them while erasing every encrypted column — write the model through its repository instead",
        M::TYPE_NAME
    )))
}

/// Refuses a partial update that names a sealed outbox column or the seal itself; erasable columns stay writable.
fn refuse_unsealing_updates<M: Model>(updates: &[(String, FilterValue)]) -> Result<(), SqlError> {
    let Some(sealed) = M::outbox_sealed_columns() else {
        return Ok(());
    };
    let refused: Vec<&str> = updates
        .iter()
        .map(|(column, _)| column.as_str())
        .filter(|column| {
            column.eq_ignore_ascii_case(OutboxColumns::SEAL)
                || sealed.iter().any(|name| name.eq_ignore_ascii_case(column))
        })
        .collect();
    if refused.is_empty() {
        return Ok(());
    }
    Err(SqlError::Config(format!(
        "columns {refused:?} of {} are covered by its outbox seal, which a partial update cannot re-seal — write the model through its repository instead",
        M::TYPE_NAME
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A staged value: two encrypted columns, sealed to its id, its owner and its freshness.
    struct Staged;

    impl Model for Staged {
        const TYPE_NAME: &'static str = "Staged";

        fn secret_columns() -> &'static [(&'static str, &'static str)] {
            &[("value", "pii"), ("salt", "pii")]
        }

        fn row_bound_columns() -> &'static [&'static str] {
            &["id", "subject_id", "confirmed_at"]
        }
    }

    struct Plain;

    impl Model for Plain {
        const TYPE_NAME: &'static str = "Plain";
    }

    fn set(column: &str, value: FilterValue) -> (String, FilterValue) {
        (column.to_owned(), value)
    }

    fn refusal(updates: &[(String, FilterValue)]) -> String {
        match refuse_unbinding_updates::<Staged>(updates) {
            Err(SqlError::Config(problem)) => problem,
            other => panic!("expected a configuration refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_value_written_into_an_encrypted_column_is_refused() {
        let problem = refusal(&[set("value", FilterValue::from("plaintext"))]);
        assert!(
            problem.contains("'value'") && problem.contains("is encrypted"),
            "{problem}"
        );
    }

    #[test]
    fn an_encrypted_column_named_in_upper_case_is_still_refused() {
        let problem = refusal(&[set("VALUE", FilterValue::from("plaintext"))]);
        assert!(problem.contains("is encrypted"), "{problem}");
    }

    #[test]
    fn changing_a_bound_column_is_refused_in_any_case() {
        for column in ["subject_id", "SUBJECT_ID"] {
            let problem = refusal(&[set(column, FilterValue::from("another-person"))]);
            assert!(
                problem.contains("bound into its encrypted values"),
                "{problem}"
            );
        }
    }

    #[test]
    fn clearing_a_bound_column_without_erasing_every_encrypted_one_is_refused() {
        let problem = refusal(&[
            set("value", FilterValue::Null),
            set("subject_id", FilterValue::Null),
        ]);
        assert!(
            problem.contains("bound into its encrypted values"),
            "{problem}"
        );
    }

    #[test]
    fn erasure_clears_every_encrypted_column_and_the_bound_ones_it_names() {
        let erasure = [
            set("value", FilterValue::Null),
            set("SALT", FilterValue::Null),
            set("subject_id", FilterValue::Null),
        ];
        assert!(refuse_unbinding_updates::<Staged>(&erasure).is_ok());
    }

    #[test]
    fn one_encrypted_column_may_be_cleared_on_its_own() {
        assert!(refuse_unbinding_updates::<Staged>(&[set("salt", FilterValue::Null)]).is_ok());
    }

    #[test]
    fn a_model_without_encrypted_columns_is_not_constrained() {
        let updates = [set("subject_id", FilterValue::from("anyone"))];
        assert!(refuse_unbinding_updates::<Plain>(&updates).is_ok());
    }

    struct Sealed;

    impl Model for Sealed {
        const TYPE_NAME: &'static str = "Sealed";

        fn outbox_sealed_columns() -> Option<&'static [&'static str]> {
            Some(&["id", "commitment"])
        }
    }

    fn unsealing_refusal(updates: &[(String, FilterValue)]) -> String {
        match refuse_unsealing_updates::<Sealed>(updates) {
            Err(SqlError::Config(problem)) => problem,
            other => panic!("expected a configuration refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_sealed_column_written_outside_the_model_is_refused_in_any_case() {
        for column in ["commitment", "COMMITMENT", "id"] {
            let problem = unsealing_refusal(&[set(column, FilterValue::from("swapped"))]);
            assert!(problem.contains("outbox seal"), "{problem}");
        }
    }

    #[test]
    fn nulling_a_sealed_column_is_refused_too() {
        let problem = unsealing_refusal(&[set("commitment", FilterValue::Null)]);
        assert!(problem.contains("outbox seal"), "{problem}");
    }

    #[test]
    fn the_seal_itself_cannot_be_written_outside_the_model() {
        let problem = unsealing_refusal(&[set("outbox_seal", FilterValue::Null)]);
        assert!(problem.contains("outbox_seal"), "{problem}");
    }

    #[test]
    fn an_erasure_and_a_retry_reset_stay_writable() {
        let updates = [
            set("subject", FilterValue::Null),
            set("attempts", FilterValue::from(0_i64)),
        ];
        assert!(refuse_unsealing_updates::<Sealed>(&updates).is_ok());
    }

    #[test]
    fn a_model_that_is_not_an_outbox_is_not_constrained_by_a_seal() {
        let updates = [set("commitment", FilterValue::from("anything"))];
        assert!(refuse_unsealing_updates::<Plain>(&updates).is_ok());
    }
}
