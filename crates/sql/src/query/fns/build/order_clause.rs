use crate::SqlError;
use crate::ident::validate_identifier;
use fundamental_db::{OrderBy, OrderDirection, OrderNulls};

/// Builds an ORDER BY clause, refusing an unsafe column identifier.
pub(crate) fn build_order_clause(order_by: &[OrderBy]) -> Result<String, SqlError> {
    let mut clauses = Vec::with_capacity(order_by.len());
    for o in order_by {
        validate_identifier(&o.column, false)?;
        let mut clause = o.column.clone();
        match o.direction {
            OrderDirection::Asc => clause.push_str(" ASC"),
            OrderDirection::Desc => clause.push_str(" DESC"),
        }
        if let Some(nulls) = &o.nulls {
            match nulls {
                OrderNulls::First => clause.push_str(" NULLS FIRST"),
                OrderNulls::Last => clause.push_str(" NULLS LAST"),
            }
        }
        clauses.push(clause);
    }
    Ok(clauses.join(", "))
}
