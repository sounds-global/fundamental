use fundamental_core::AnyError;
use fundamental_core::validate::ValidationErrors;
use fundamental_db::DatabaseError;
use fundamental_macros::Error;
use fundamental_sql::SqlError;

/// Why a job could not be scheduled, moved, cancelled or fired.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum JobError {
    /// A subject outside `[A-Za-z0-9._:/-]{1,256}`. The subject itself is not carried: it names
    /// an occurrence of someone's business, and this error reaches a log.
    #[error(
        code = "JOBS_001",
        message = "A job subject is not valid",
        developer_message = "a subject of {length} characters is not a job subject: [A-Za-z0-9._:/-]{{1,256}}",
        status = 500
    )]
    InvalidSubject {
        /// How many characters the refused subject had.
        length: usize,
    },

    /// The kinds an application declared are not a valid set; every problem is collected.
    #[error(
        code = "JOBS_002",
        message = "The job kinds are not valid",
        developer_message = "job kinds refused: {problems}",
        status = 500
    )]
    InvalidKinds {
        /// Every problem, by kind.
        problems: ValidationErrors,
    },

    /// A due time outside what a job may hold: before the tombstone window, whose occurrences
    /// may already have fired and been purged, or outside the years 2000 to 2200.
    #[error(
        code = "JOBS_003",
        message = "A job's due time is out of range",
        developer_message = "job `{kind}` due {due} is out of range: {reason}",
        status = 500
    )]
    DueOutOfRange {
        /// Which kind.
        kind: String,
        /// The due time refused.
        due: String,
        /// Which bound it broke.
        reason: &'static str,
    },

    /// The job table did not answer.
    #[error(
        code = "JOBS_004",
        message = "Scheduled jobs are unavailable",
        developer_message = "the job table could not be read or written: {cause}",
        status = 503,
        retryable
    )]
    Unavailable {
        /// The store's own error.
        #[source]
        cause: DatabaseError,
    },

    /// The engine's hybrid clock could not be read, so no due time can be judged.
    #[error(
        code = "JOBS_005",
        message = "The job clock is unavailable",
        developer_message = "the cluster clock could not be read: {cause}",
        status = 503,
        retryable
    )]
    ClockUnavailable {
        /// The store's own error.
        #[source]
        cause: SqlError,
    },

    /// The job table does not exist: the app has not registered the crate's migrations.
    #[error(
        code = "JOBS_006",
        message = "Scheduled jobs are not set up",
        developer_message = "table fundamental_jobs is missing; register \
                             fundamental_jobs::JOBS_MIGRATIONS with MigrateRunner::with_embedded",
        status = 500
    )]
    TableMissing,

    /// A job write's commit was sent and its answer lost; whether it applied is unknown.
    #[error(
        code = "JOBS_007",
        message = "We could not confirm whether the job changed",
        developer_message = "job commit outcome unknown",
        status = 500
    )]
    CommitOutcomeUnknown,

    /// A stored job names a kind this application does not declare.
    #[error(
        code = "JOBS_008",
        message = "A job's kind is not known",
        developer_message = "job kind `{kind}` is not one this application declares",
        status = 500
    )]
    UnknownKind {
        /// The kind stored.
        kind: String,
    },

    /// A stored job's id does not derive from its kind and subject.
    #[error(
        code = "JOBS_009",
        message = "A job record is damaged",
        developer_message = "job `{job_id}` does not derive from its kind and subject",
        status = 500
    )]
    IdentityMismatch {
        /// The id stored.
        job_id: String,
    },

    /// Job settings out of range; every problem is collected.
    #[error(
        code = "JOBS_010",
        message = "The job settings are not valid",
        developer_message = "job settings refused: {problems}",
        status = 500
    )]
    InvalidSettings {
        /// Every problem, by field.
        problems: ValidationErrors,
    },

    /// The handler refused or failed to fire a job; its transaction rolled back, so nothing it
    /// wrote committed.
    #[error(
        code = "JOBS_011",
        message = "A scheduled job failed with {handler_code}",
        developer_message = "job `{kind}` handler failed with {handler_code}: {cause}",
        status = 500
    )]
    HandlerFailed {
        /// Which kind.
        kind: String,
        /// The handler's own error code.
        handler_code: String,
        /// The handler's own error.
        #[source]
        cause: AnyError,
    },

    /// Scheduling raced other writers of the same job past its retry budget.
    #[error(
        code = "JOBS_012",
        message = "A job is being changed concurrently",
        developer_message = "job `{kind}` for one subject kept changing under the schedule",
        status = 409,
        retryable
    )]
    Contended {
        /// Which kind.
        kind: String,
    },

    /// A handler postponed a job to an instant not after the database's clock in the fire
    /// transaction, or outside the years 2000 to 2200. The attempt rolls back as a failure.
    #[error(
        code = "JOBS_013",
        message = "A scheduled job postponed itself to an invalid time",
        developer_message = "job `{kind}` postponed to {not_before}, which is {reason}",
        status = 500
    )]
    PostponeRefused {
        /// Which kind.
        kind: String,
        /// The instant the handler answered.
        not_before: String,
        /// Which bound it broke.
        reason: &'static str,
    },

    /// A handler's verdict could not be written: the job row no longer carried the fired mark
    /// this transaction wrote, or the next attempt is not representable. The attempt rolls back.
    #[error(
        code = "JOBS_014",
        message = "A scheduled job's verdict could not be recorded",
        developer_message = "job `{job_id}` verdict not recorded: {reason}",
        status = 500
    )]
    VerdictNotRecorded {
        /// The job.
        job_id: String,
        /// Why.
        reason: &'static str,
    },

    /// A drain was confined to the nil id, which subjects about no one in particular name.
    #[error(
        code = "JOBS_015",
        message = "A job drain cannot be confined to no one",
        developer_message = "a drain confined to the nil id would fire every job about no one in particular",
        status = 500
    )]
    NobodyToConfineTo,

    /// A store error, as it arrives: one raised by the caller's own transaction's begin or commit.
    #[error(transparent)]
    Store(#[from] DatabaseError),
}

impl JobError {
    /// This error, with a bare store error classified: a missing table, a lost commit answer,
    /// or the store not answering.
    pub(crate) fn about(self) -> Self {
        let Self::Store(cause) = self else {
            return self;
        };
        let undefined_table = match &cause {
            DatabaseError::Other(any) => any
                .downcast_ref::<SqlError>()
                .is_some_and(SqlError::is_undefined_table),
            _ => false,
        };
        if undefined_table {
            Self::TableMissing
        } else if matches!(cause, DatabaseError::CommitOutcomeUnknown { .. }) {
            Self::CommitOutcomeUnknown
        } else {
            Self::Unavailable { cause }
        }
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[derive(Debug, Error)]
    enum Refused {
        #[error(code = "APP_001", message = "refused", status = 409)]
        Hold,
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    fn every_variant_carries_its_own_code() {
        let cases: Vec<(JobError, &str, u16)> = vec![
            (JobError::InvalidSubject { length: 3 }, "JOBS_001", 500),
            (
                JobError::InvalidKinds {
                    problems: ValidationErrors::new(),
                },
                "JOBS_002",
                500,
            ),
            (
                JobError::DueOutOfRange {
                    kind: "hold.lapse".to_owned(),
                    due: "1999-01-01".to_owned(),
                    reason: "before 2000",
                },
                "JOBS_003",
                500,
            ),
            (JobError::TableMissing, "JOBS_006", 500),
            (JobError::CommitOutcomeUnknown, "JOBS_007", 500),
            (
                JobError::UnknownKind {
                    kind: "x".to_owned(),
                },
                "JOBS_008",
                500,
            ),
            (
                JobError::IdentityMismatch {
                    job_id: "x".to_owned(),
                },
                "JOBS_009",
                500,
            ),
            (
                JobError::InvalidSettings {
                    problems: ValidationErrors::new(),
                },
                "JOBS_010",
                500,
            ),
            (
                JobError::HandlerFailed {
                    kind: "hold.lapse".to_owned(),
                    handler_code: "APP_001".to_owned(),
                    cause: AnyError::new(Refused::Hold),
                },
                "JOBS_011",
                500,
            ),
            (
                JobError::Contended {
                    kind: "hold.lapse".to_owned(),
                },
                "JOBS_012",
                409,
            ),
            (
                JobError::PostponeRefused {
                    kind: "hold.lapse".to_owned(),
                    not_before: "2026-10-01T00:00:00Z".to_owned(),
                    reason: "not after the database's clock",
                },
                "JOBS_013",
                500,
            ),
            (
                JobError::VerdictNotRecorded {
                    job_id: "x".to_owned(),
                    reason: "the fired mark is gone",
                },
                "JOBS_014",
                500,
            ),
            (JobError::NobodyToConfineTo, "JOBS_015", 500),
        ];
        for (error, code, status) in cases {
            assert_eq!(error.error_code(), code);
            assert_eq!(error.http_status_code(), status);
        }
    }

    #[test]
    fn a_handler_failure_names_the_handler_code_and_nothing_it_carried() {
        let error = JobError::HandlerFailed {
            kind: "hold.lapse".to_owned(),
            handler_code: "APP_001".to_owned(),
            cause: AnyError::new(Refused::Hold),
        };
        assert_eq!(error.to_string(), "A scheduled job failed with APP_001");
        assert!(
            !error.is_retryable(),
            "a failed handler is retried after its backoff, never in process"
        );
    }

    #[test]
    fn a_store_error_that_is_not_a_missing_table_is_the_store_unavailable() {
        let error = JobError::Store(DatabaseError::Other(AnyError::new(Refused::Hold))).about();
        assert_eq!(error.error_code(), "JOBS_004");
        assert!(error.is_retryable());
    }
}
