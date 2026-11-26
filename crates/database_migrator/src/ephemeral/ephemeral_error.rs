//! [`EphemeralError`], the failure type for provisioning a throwaway test database.

use fundamental_db::ephemeral::EphemeralNameError;
use fundamental_macros::Error;
#[cfg(feature = "sql")]
use fundamental_sql::test_support::ReclaimedSchemaError;

/// A failure while provisioning or migrating an ephemeral test database.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EphemeralError {
    /// The caller's name prefix cannot mint a usable store name.
    #[error(
        code = "EPHEMERAL_005",
        message = "'{prefix}' cannot name an ephemeral store: {detail}",
        status = 500
    )]
    InvalidPrefix {
        /// The rejected prefix.
        prefix: String,
        /// Which of the three rules it broke.
        detail: String,
    },

    /// The database cluster could not be reached at all.
    ///
    /// The skip signal: raised by the bootstrap connection, before anything is created.
    #[error(
        code = "EPHEMERAL_001",
        message = "no database cluster reachable: {detail}",
        status = 503,
        retryable = true
    )]
    Unreachable {
        /// The underlying connection failure.
        detail: String,
    },

    /// The cluster was reachable but the `CREATE DATABASE` / `CREATE KEYSPACE`
    /// statement failed.
    #[error(
        code = "EPHEMERAL_002",
        message = "could not create ephemeral store '{name}': {detail}",
        status = 500
    )]
    Create {
        /// The name that could not be created.
        name: String,
        /// The underlying failure.
        detail: String,
    },

    /// The store was created but a pool could not be opened against it.
    #[error(
        code = "EPHEMERAL_003",
        message = "could not connect to ephemeral store '{name}': {detail}",
        status = 500
    )]
    Connect {
        /// The store that could not be connected to.
        name: String,
        /// The underlying failure.
        detail: String,
    },

    /// The shipped migrations could not be applied to the fresh store.
    ///
    /// Never a skip: a broken migration must surface here.
    #[error(
        code = "EPHEMERAL_004",
        message = "could not migrate ephemeral store '{name}': {detail}",
        status = 500
    )]
    Migrate {
        /// The store whose migrations failed.
        name: String,
        /// The underlying failure.
        detail: String,
    },
}

impl EphemeralError {
    /// Whether this failure means "no cluster here" rather than "provisioning broke".
    #[must_use]
    pub fn is_unreachable(&self) -> bool {
        matches!(self, Self::Unreachable { .. })
    }
}

/// Naming refusals arrive as provisioning refusals, keeping the reason.
impl From<EphemeralNameError> for EphemeralError {
    fn from(error: EphemeralNameError) -> Self {
        Self::InvalidPrefix {
            prefix: error.prefix().to_owned(),
            detail: error.detail(),
        }
    }
}

/// A held claim's refusals as provisioning refusals; an unreachable database is still a skip.
#[cfg(feature = "sql")]
impl From<ReclaimedSchemaError> for EphemeralError {
    fn from(error: ReclaimedSchemaError) -> Self {
        match error {
            ReclaimedSchemaError::InvalidName { prefix, detail, .. } => {
                Self::InvalidPrefix { prefix, detail }
            }
            ReclaimedSchemaError::Unreachable { detail, .. } => Self::Unreachable { detail },
            ReclaimedSchemaError::NotHeld { name, detail } => Self::Create { name, detail },
            other => Self::Create {
                name: String::new(),
                detail: other.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::error::Error as _;

    #[test]
    fn unreachable_is_the_only_skip_signal() {
        let unreachable = EphemeralError::Unreachable {
            detail: "connection refused".to_owned(),
        };
        assert!(unreachable.is_unreachable());

        for other in [
            EphemeralError::Create {
                name: "t".to_owned(),
                detail: "boom".to_owned(),
            },
            EphemeralError::Connect {
                name: "t".to_owned(),
                detail: "boom".to_owned(),
            },
            EphemeralError::Migrate {
                name: "t".to_owned(),
                detail: "boom".to_owned(),
            },
            EphemeralError::InvalidPrefix {
                detail: "not a plain identifier".to_owned(),
                prefix: "x; DROP DATABASE y".to_owned(),
            },
        ] {
            assert!(!other.is_unreachable(), "{other:?} must not read as a skip");
        }
    }

    #[test]
    fn each_variant_carries_its_stable_code_and_status() {
        let unreachable = EphemeralError::Unreachable {
            detail: "refused".to_owned(),
        };
        assert_eq!(unreachable.error_code(), "EPHEMERAL_001");
        assert_eq!(unreachable.http_status_code(), 503);
        assert!(unreachable.is_retryable());

        let migrate = EphemeralError::Migrate {
            name: "layer2_test_abc".to_owned(),
            detail: "syntax error".to_owned(),
        };
        assert_eq!(migrate.error_code(), "EPHEMERAL_004");
        assert_eq!(migrate.http_status_code(), 500);
        assert!(!migrate.is_retryable());
        assert!(migrate.to_string().contains("layer2_test_abc"));
    }
}
