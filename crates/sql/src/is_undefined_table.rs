use crate::SqlError;
use fundamental_db::DatabaseError;

/// Whether `error` is the engine saying a table does not exist, including a `SqlError` erased into `Other`.
#[must_use]
pub fn is_undefined_table(error: &DatabaseError) -> bool {
    match error {
        DatabaseError::Other(any) => any
            .downcast_ref::<SqlError>()
            .is_some_and(SqlError::is_undefined_table),
        _ => false,
    }
}
