use fundamental_db::ephemeral::EphemeralNameError;
use fundamental_macros::Error;

/// Why [`ReclaimedSchema::claim`](super::ReclaimedSchema::claim) refused; a suite skips on [`Unreachable`](Self::Unreachable).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ReclaimedSchemaError {
    /// The prefix cannot mint a held name, decided before any connection is opened.
    #[error(
        code = "RECLAIMED_SCHEMA_001",
        message = "Something went wrong while preparing a test fixture.",
        developer_message = "the schema prefix `{prefix}` cannot mint a held name: {detail}",
        status = 500
    )]
    InvalidName {
        /// The prefix as it was offered.
        prefix: String,
        /// Which rule it broke.
        detail: String,
        /// The naming refusal itself.
        #[source]
        source: EphemeralNameError,
    },

    /// The holder could not connect to the database the schema is to live in.
    #[error(
        code = "RECLAIMED_SCHEMA_002",
        message = "Something went wrong while preparing a test fixture.",
        developer_message = "the holder for schema `{name}` could not connect: {detail}",
        status = 500
    )]
    Unreachable {
        /// The name that was to be held.
        name: String,
        /// The connection failure.
        detail: String,
    },

    /// The lock that makes the schema reclaimable could not be taken; a claim never proceeds without it.
    #[error(
        code = "RECLAIMED_SCHEMA_003",
        message = "Something went wrong while preparing a test fixture.",
        developer_message = "the lock for schema `{name}` could not be taken: {detail}",
        status = 500
    )]
    NotHeld {
        /// The last name that was tried.
        name: String,
        /// Why the lock was not taken.
        detail: String,
    },
}

impl ReclaimedSchemaError {
    /// Whether this failure means "no database here" rather than "the fixture broke".
    #[must_use]
    pub fn is_unreachable(&self) -> bool {
        matches!(self, Self::Unreachable { .. })
    }
}

impl From<EphemeralNameError> for ReclaimedSchemaError {
    fn from(source: EphemeralNameError) -> Self {
        Self::InvalidName {
            prefix: source.prefix().to_owned(),
            detail: source.detail(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn a_naming_refusal_keeps_its_prefix_and_its_rule() {
        let error = ReclaimedSchemaError::from(EphemeralNameError::PrefixNotSeparated {
            prefix: "s".to_owned(),
        });
        assert!(matches!(
            &error,
            ReclaimedSchemaError::InvalidName { prefix, detail, .. }
                if prefix == "s" && detail.contains("must end with `_`")
        ));
        assert_eq!(error.error_code(), "RECLAIMED_SCHEMA_001");
        assert_eq!(error.http_status_code(), 500);
        assert!(!error.is_unreachable());
    }

    #[test]
    fn only_an_unreachable_database_reads_as_unreachable() {
        let unreachable = ReclaimedSchemaError::Unreachable {
            name: "s_1_h".to_owned(),
            detail: "connection refused".to_owned(),
        };
        assert!(unreachable.is_unreachable());
        assert_eq!(unreachable.error_code(), "RECLAIMED_SCHEMA_002");

        let not_held = ReclaimedSchemaError::NotHeld {
            name: "s_1_h".to_owned(),
            detail: "already held".to_owned(),
        };
        assert!(!not_held.is_unreachable());
        assert_eq!(not_held.error_code(), "RECLAIMED_SCHEMA_003");
    }
}
