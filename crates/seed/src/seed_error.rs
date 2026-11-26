use fundamental_config::ConfigError;
use fundamental_core::AnyError;
use fundamental_db::DatabaseError;
use fundamental_macros::Error;

/// An error from running a seed.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SeedError {
    /// A lower-layer Fundamental error with no more specific variant, preserved losslessly.
    Other(AnyError),

    /// A database operation failed while seeding; its code and status are forwarded unchanged.
    #[error(transparent)]
    Database(#[from] DatabaseError),

    /// A seed reported a failure that does not map onto a more specific variant.
    #[error(code = "SEED_001", message = "Seed failed: {message}", status = 500)]
    Message {
        /// The failure description supplied by the seed.
        message: String,
    },

    /// The `after` dependencies form a cycle, so no run order exists.
    #[error(
        code = "SEED_002",
        message = "Circular dependency detected among seeds: {seeds}",
        status = 500
    )]
    CircularDependency {
        /// The comma-separated names of the seeds involved in the cycle.
        seeds: String,
    },

    /// Seeding was refused because the active environment is not allowed.
    #[error(
        code = "SEED_003",
        message = "Seeding is refused in the {environment} environment; use the release override to proceed",
        status = 403
    )]
    EnvironmentNotAllowed {
        /// The active environment that was refused (e.g. `production`, or a custom name).
        environment: String,
    },

    /// The environment could not be resolved; the configuration's own error, unchanged.
    #[error(transparent)]
    Config(#[from] ConfigError),
}

impl SeedError {
    /// Creates a [`SeedError::Message`] from any displayable value.
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Message {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    #[test]
    fn message_carries_stable_code_and_status() {
        let err = SeedError::msg("could not load fixture");
        assert_eq!(err.error_code(), "SEED_001");
        assert_eq!(err.http_status_code(), 500);
        assert!(!err.is_retryable());
        assert!(err.to_string().contains("could not load fixture"));
    }

    #[test]
    fn circular_dependency_names_the_seeds() {
        let err = SeedError::CircularDependency {
            seeds: "a, b".into(),
        };
        assert_eq!(err.error_code(), "SEED_002");
        assert_eq!(err.http_status_code(), 500);
        assert!(err.to_string().contains("a, b"));
    }

    #[test]
    fn environment_not_allowed_is_forbidden() {
        let err = SeedError::EnvironmentNotAllowed {
            environment: "production".into(),
        };
        assert_eq!(err.error_code(), "SEED_003");
        assert_eq!(err.http_status_code(), 403);
        assert!(err.to_string().contains("production"));
    }

    #[test]
    fn other_catch_all_delegates_to_the_wrapped_error() {
        let err: SeedError = AnyError::new(DatabaseError::internal("boom")).into();
        assert_eq!(err.error_code(), "DB_011");
        assert_eq!(err.http_status_code(), 500);
    }

    #[test]
    fn database_failure_forwards_transparently() {
        let err: SeedError = DatabaseError::unique_violation("users_email_key").into();
        assert_eq!(err.error_code(), "DB_003");
        assert_eq!(err.http_status_code(), 409);
        assert!(matches!(err, SeedError::Database(_)));
    }
}
