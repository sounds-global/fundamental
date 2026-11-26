use fundamental_core::validate::ValidationErrors;
use fundamental_db::DatabaseError;
use fundamental_macros::Error;
use fundamental_sql::SqlError;

/// Why a lease could not be taken, kept, released or fenced on.
///
/// Losing a lease to another instance is not an error: see [`Acquisition::HeldElsewhere`](crate::Acquisition::HeldElsewhere).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LeaseError {
    /// The lease's store did not answer.
    #[error(
        code = "SLEASE_001",
        message = "Singleton lease unavailable",
        developer_message = "lease `{lease}` could not be read or written: {cause}",
        status = 503,
        retryable
    )]
    Unavailable {
        /// Which lease.
        lease: String,
        /// The store's own error.
        #[source]
        cause: DatabaseError,
    },

    /// The lease has passed to another holder or expired: a fenced write is refused.
    #[error(
        code = "SLEASE_002",
        message = "This work no longer holds its lease",
        developer_message = "lease `{lease}` at epoch {epoch} was superseded",
        status = 409
    )]
    Superseded {
        /// Which lease.
        lease: String,
        /// The epoch the refused work held.
        epoch: i64,
    },

    /// A lease name outside `[a-z][a-z0-9-]{0,62}`.
    #[error(
        code = "SLEASE_003",
        message = "A lease name is not valid",
        developer_message = "`{name}` is not a lease name: [a-z][a-z0-9-]{{0,62}}",
        status = 500
    )]
    InvalidName {
        /// The name refused.
        name: String,
    },

    /// Lease settings out of range; every problem is collected.
    #[error(
        code = "SLEASE_004",
        message = "The lease settings are not valid",
        developer_message = "lease settings refused: {problems}",
        status = 500
    )]
    InvalidConfig {
        /// Every problem, by field.
        problems: ValidationErrors,
    },

    /// The engine's hybrid clock could not be read, so no expiry can be judged.
    #[error(
        code = "SLEASE_005",
        message = "The lease clock is unavailable",
        developer_message = "lease `{lease}` could not read the cluster clock: {cause}",
        status = 503,
        retryable
    )]
    ClockUnavailable {
        /// Which lease.
        lease: String,
        /// The store's own error.
        #[source]
        cause: DatabaseError,
    },

    /// A stored lease row breaks the shape its table's constraints guarantee.
    #[error(
        code = "SLEASE_006",
        message = "A lease record is damaged",
        developer_message = "lease `{lease}` row is inconsistent: {detail}",
        status = 500
    )]
    CorruptRow {
        /// Which lease.
        lease: String,
        /// What is wrong with it.
        detail: String,
    },

    /// The fencing epoch cannot grow any further.
    #[error(
        code = "SLEASE_007",
        message = "A lease cannot be taken again",
        developer_message = "lease `{lease}` epoch is at its maximum",
        status = 500
    )]
    EpochExhausted {
        /// Which lease.
        lease: String,
    },

    /// A lease surface's supervisor stopped on its own.
    #[error(
        code = "SLEASE_008",
        message = "Singleton work stopped unexpectedly",
        developer_message = "lease `{lease}` surface ended: {detail}",
        status = 500
    )]
    SurfaceEnded {
        /// Which lease.
        lease: String,
        /// Why.
        detail: String,
    },

    /// The lease tables do not exist: the app has not registered the crate's migrations.
    #[error(
        code = "SLEASE_009",
        message = "Singleton work is not set up",
        developer_message = "lease `{lease}`: table fundamental_leases is missing; register \
                             fundamental_lease::LEASE_MIGRATIONS with MigrateRunner::with_embedded",
        status = 500
    )]
    TableMissing {
        /// Which lease.
        lease: String,
    },

    /// A lease write's commit was sent and its answer lost; whether it applied is unknown.
    #[error(
        code = "SLEASE_010",
        message = "We could not confirm whether the lease changed",
        developer_message = "lease `{lease}`: commit outcome unknown",
        status = 500
    )]
    CommitOutcomeUnknown {
        /// Which lease.
        lease: String,
    },

    /// A work key outside `[A-Za-z0-9._:/-]{1,200}`.
    #[error(
        code = "SLEASE_011",
        message = "A work key is not valid",
        developer_message = "`{key}` is not a work key: [A-Za-z0-9._:/-]{{1,200}}",
        status = 500
    )]
    InvalidWorkKey {
        /// The key refused, cut to its first 200 characters.
        key: String,
    },

    /// A store error raised before any lease context existed.
    #[error(transparent)]
    Store(#[from] DatabaseError),
}

impl LeaseError {
    /// This error, with a bare store error classified for `lease`.
    pub(crate) fn about(self, lease: &str) -> Self {
        let Self::Store(cause) = self else {
            return self;
        };
        let undefined_table = match &cause {
            DatabaseError::Other(any) => any
                .downcast_ref::<SqlError>()
                .is_some_and(SqlError::is_undefined_table),
            _ => false,
        };
        let lease = lease.to_owned();
        if undefined_table {
            Self::TableMissing { lease }
        } else if matches!(cause, DatabaseError::CommitOutcomeUnknown { .. }) {
            Self::CommitOutcomeUnknown { lease }
        } else {
            Self::Unavailable { lease, cause }
        }
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn every_variant_carries_its_own_code() {
        let lease = || "keeper".to_owned();
        let variants = [
            LeaseError::Unavailable {
                lease: lease(),
                cause: DatabaseError::timeout(1),
            },
            LeaseError::Superseded {
                lease: lease(),
                epoch: 1,
            },
            LeaseError::InvalidName { name: lease() },
            LeaseError::InvalidConfig {
                problems: ValidationErrors::new(),
            },
            LeaseError::ClockUnavailable {
                lease: lease(),
                cause: DatabaseError::timeout(1),
            },
            LeaseError::CorruptRow {
                lease: lease(),
                detail: String::new(),
            },
            LeaseError::EpochExhausted { lease: lease() },
            LeaseError::SurfaceEnded {
                lease: lease(),
                detail: String::new(),
            },
            LeaseError::TableMissing { lease: lease() },
            LeaseError::CommitOutcomeUnknown { lease: lease() },
            LeaseError::InvalidWorkKey { key: String::new() },
        ];
        let codes: Vec<&str> = variants.iter().map(|error| error.error_code()).collect();
        assert_eq!(
            codes,
            [
                "SLEASE_001",
                "SLEASE_002",
                "SLEASE_003",
                "SLEASE_004",
                "SLEASE_005",
                "SLEASE_006",
                "SLEASE_007",
                "SLEASE_008",
                "SLEASE_009",
                "SLEASE_010",
                "SLEASE_011"
            ]
        );
        assert!(
            variants[0].is_retryable(),
            "an unreachable store is retried next renewal"
        );
        assert!(
            !variants[1].is_retryable(),
            "a superseded tenure never retries its write"
        );
    }

    #[test]
    fn a_lost_commit_answer_is_named_for_its_lease() {
        let error =
            LeaseError::Store(DatabaseError::commit_outcome_unknown("reset")).about("keeper");
        assert_eq!(error.error_code(), "SLEASE_010");
        let error = LeaseError::Store(DatabaseError::timeout(5)).about("keeper");
        assert_eq!(error.error_code(), "SLEASE_001");
    }
}
