use fundamental_core::model::CompositeKeyModel;
use fundamental_db::{DatabaseError, DatabaseResult};

use crate::ident::validate_identifier;
use crate::query::BindKey;

/// Builds `k1 = $1 AND k2 = $2 …` over the model's validated `KEY_COLUMNS`, in key order.
///
/// # Errors
///
/// Returns [`DatabaseError`] if a key column is not a valid identifier, or if the model's
/// declared key columns and its key-value tuple disagree about how many columns the key has.
pub(crate) fn key_predicate<'q, M>() -> DatabaseResult<String>
where
    M: CompositeKeyModel,
    M::KeyValues: BindKey<'q>,
{
    let columns = M::KEY_COLUMNS;
    let arity = <M::KeyValues as BindKey<'q>>::ARITY;
    if columns.len() != arity {
        return Err(DatabaseError::query(format!(
            "`{}` declares {} key column(s) ({}) but its key tuple binds {arity} — the derive \
             and the model disagree about what addresses a row",
            M::TABLE,
            columns.len(),
            columns.join(", "),
        )));
    }

    let mut predicate = String::new();
    for (at, column) in columns.iter().enumerate() {
        validate_identifier(column, false).map_err(|e| DatabaseError::query(e.to_string()))?;
        if at > 0 {
            predicate.push_str(" AND ");
        }
        predicate.push_str(column);
        predicate.push_str(" = $");
        predicate.push_str(&(at + 1).to_string());
    }
    Ok(predicate)
}
