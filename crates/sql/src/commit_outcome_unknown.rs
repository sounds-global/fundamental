use fundamental_db::DatabaseError;

use crate::SqlError;

/// Turns a commit failure that lost the server's answer into
/// [`DatabaseError::CommitOutcomeUnknown`], and returns every other failure unchanged.
#[must_use]
pub fn commit_outcome_unknown(error: DatabaseError) -> DatabaseError {
    let lost = match &error {
        DatabaseError::Other(any) => any
            .downcast_ref::<SqlError>()
            .is_some_and(SqlError::loses_the_answer),
        _ => false,
    };
    if lost {
        DatabaseError::commit_outcome_unknown(error.to_string())
    } else {
        error
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::{AnyError, Error as _};

    use super::*;

    fn wrapped(error: SqlError) -> DatabaseError {
        DatabaseError::Other(AnyError::new(error))
    }

    #[test]
    fn a_dropped_connection_at_commit_is_an_unknown_outcome() {
        let io = std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "connection closed");
        let error = commit_outcome_unknown(wrapped(SqlError::Sqlx(sqlx::Error::Io(io))));
        assert_eq!(error.error_code(), "DB_012");
        assert!(!error.is_retryable());
    }

    #[test]
    fn a_closed_pool_is_not_mistaken_for_a_lost_answer() {
        let error = commit_outcome_unknown(wrapped(SqlError::Sqlx(sqlx::Error::PoolClosed)));
        assert_ne!(error.error_code(), "DB_012");
    }

    #[test]
    fn a_conflict_the_server_reported_stays_a_retryable_conflict() {
        let error = commit_outcome_unknown(wrapped(SqlError::serialization_conflict("40001")));
        assert_ne!(error.error_code(), "DB_012");
        assert!(
            error.is_retryable(),
            "the server answered: nothing was applied"
        );
    }

    #[test]
    fn an_error_that_is_not_the_drivers_is_left_alone() {
        let error = commit_outcome_unknown(DatabaseError::timeout(5));
        assert_eq!(error.error_code(), "DB_008");
    }
}
