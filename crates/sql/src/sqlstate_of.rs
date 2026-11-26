use crate::SqlError;
use fundamental_db::DatabaseError;

/// The SQLSTATE class a [`DatabaseError`] came from, including a `SqlError` erased into [`Other`](DatabaseError::Other).
#[must_use]
pub fn sqlstate_of(error: &DatabaseError) -> Option<&'static str> {
    match error {
        DatabaseError::Other(any) => any.downcast_ref::<SqlError>().and_then(SqlError::sqlstate),
        _ => None,
    }
}
