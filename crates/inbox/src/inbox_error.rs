//! [`InboxError`] — why the inbox could not start, record, or refused an event.

use fundamental_core::validate::ValidationErrors;
use fundamental_db::DatabaseError;
use fundamental_macros::Error;
use fundamental_nats::NatsError;
use fundamental_sql::is_undefined_table;

/// Why the inbox could not start or reach its store, and why it refused an event.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum InboxError {
    /// The inbox's store did not answer.
    #[error(
        code = "SINBOX_001",
        message = "Event inbox unavailable",
        developer_message = "the inbox store could not be read or written: {cause}",
        status = 503,
        retryable
    )]
    Unavailable {
        /// The store's own error.
        #[source]
        cause: DatabaseError,
    },

    /// The inbox tables are not in the database.
    #[error(
        code = "SINBOX_002",
        message = "The event inbox is not set up",
        developer_message = "the inbox tables are missing: register INBOX_MIGRATIONS with the app's migrator (`migrate_main!(.., embedded = [&INBOX_MIGRATIONS])`) and migrate",
        status = 500
    )]
    TableMissing,

    /// Inbox settings out of range; every problem is collected.
    #[error(
        code = "SINBOX_003",
        message = "The inbox settings are not valid",
        developer_message = "invalid [inbox] settings: {problems}",
        status = 500
    )]
    InvalidSettings {
        /// Every value out of range.
        problems: ValidationErrors,
    },

    /// The consumer is set up so the inbox cannot keep its promise.
    #[error(
        code = "SINBOX_004",
        message = "An inbox consumer is not set up safely",
        developer_message = "consumer `{consumer}` on stream `{stream}`: {problems}",
        status = 500
    )]
    UnsafeConsumer {
        /// The stream.
        stream: String,
        /// The consumer.
        consumer: String,
        /// Every problem found, joined.
        problems: String,
    },

    /// An effect name already claimed — by another listener in this inbox, or by a different
    /// handler anywhere on this database.
    #[error(
        code = "SINBOX_005",
        message = "An effect is declared twice",
        developer_message = "effect `{effect}`: {detail}",
        status = 500
    )]
    EffectDeclaredTwice {
        /// The effect.
        effect: String,
        /// Who holds it, and what to do.
        detail: String,
    },

    /// An event whose identity the inbox cannot use.
    #[error(
        code = "SINBOX_006",
        message = "An event carries an invalid identity",
        developer_message = "effect `{effect}`: {detail}",
        status = 422
    )]
    InvalidEventId {
        /// The effect it was delivered to.
        effect: String,
        /// What was wrong with it.
        detail: String,
    },

    /// An event id already applied, now carrying different content.
    #[error(
        code = "SINBOX_007",
        message = "An event identity was reused for different content",
        developer_message = "effect `{effect}`: an applied event id arrived with a different payload digest; the publisher reuses ids",
        status = 409
    )]
    EventIdReused {
        /// The effect.
        effect: String,
    },

    /// An event too old, or too far in the future, to be vouched for.
    #[error(
        code = "SINBOX_008",
        message = "An event is outside the inbox's window",
        developer_message = "effect `{effect}`: {detail}",
        status = 422
    )]
    BeyondHorizon {
        /// The effect.
        effect: String,
        /// How far outside, and which way.
        detail: String,
    },

    /// The handler panicked.
    #[error(
        code = "SINBOX_009",
        message = "An event handler failed unexpectedly",
        developer_message = "effect `{effect}`: the handler panicked; its transaction was rolled back",
        status = 500
    )]
    HandlerPanicked {
        /// The effect.
        effect: String,
    },

    /// The effect outlived its deadline.
    #[error(
        code = "SINBOX_010",
        message = "An event took too long to process",
        developer_message = "effect `{effect}`: abandoned after {deadline_secs}s; its transaction was rolled back",
        status = 504,
        retryable
    )]
    DeadlineExceeded {
        /// The effect.
        effect: String,
        /// The deadline it outlived.
        deadline_secs: u64,
    },

    /// An effect name outside `[a-z0-9][a-z0-9._-]{0,99}`.
    #[error(
        code = "SINBOX_011",
        message = "An effect name is not valid",
        developer_message = "`{name}` is not an effect name: [a-z0-9][a-z0-9._-]{{0,99}}",
        status = 500
    )]
    InvalidEffectName {
        /// The name refused.
        name: String,
    },

    /// The broker could not be asked about, or deliver from, a consumer.
    #[error(
        code = "SINBOX_012",
        message = "Event broker unavailable",
        developer_message = "the broker could not be reached: {cause}",
        status = 503,
        retryable
    )]
    Broker {
        /// The broker's own error.
        #[source]
        cause: NatsError,
    },

    /// No parked record has that address.
    #[error(
        code = "SINBOX_013",
        message = "No such parked event",
        developer_message = "no parked record at {address}",
        status = 404
    )]
    ParkedNotFound {
        /// The address asked for.
        address: String,
    },

    /// A payload that does not decode as the handler's event.
    #[error(
        code = "SINBOX_015",
        message = "An event could not be read",
        developer_message = "effect `{effect}`: the payload does not decode: {detail}",
        status = 422
    )]
    Undecodable {
        /// The effect.
        effect: String,
        /// The decoder's own error.
        detail: String,
    },

    /// No effect has that name on this database.
    #[error(
        code = "SINBOX_016",
        message = "No such effect",
        developer_message = "no effect named `{effect}` has been claimed on this database",
        status = 404
    )]
    EffectNotFound {
        /// The effect asked for.
        effect: String,
    },

    /// A parked record's address that does not parse.
    #[error(
        code = "SINBOX_014",
        message = "A parked event's address is not valid",
        developer_message = "`{address}` is not <effect>/<stream>/<created-at>/<sequence>: {detail}",
        status = 400
    )]
    InvalidParkedAddress {
        /// The address given.
        address: String,
        /// What was wrong.
        detail: String,
    },
}

impl InboxError {
    /// Classifies a store error: an undefined table is a set-up problem, anything else the
    /// store being unavailable.
    #[must_use]
    pub(crate) fn from_store(cause: DatabaseError) -> Self {
        if is_undefined_table(&cause) {
            Self::TableMissing
        } else {
            Self::Unavailable { cause }
        }
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    fn every_variant_carries_its_own_code_and_status() {
        let cases = [
            (
                InboxError::Unavailable {
                    cause: DatabaseError::internal("down"),
                },
                "SINBOX_001",
                503,
                true,
            ),
            (InboxError::TableMissing, "SINBOX_002", 500, false),
            (
                InboxError::UnsafeConsumer {
                    stream: "s".into(),
                    consumer: "c".into(),
                    problems: "p".into(),
                },
                "SINBOX_004",
                500,
                false,
            ),
            (
                InboxError::EffectDeclaredTwice {
                    effect: "e".into(),
                    detail: "d".into(),
                },
                "SINBOX_005",
                500,
                false,
            ),
            (
                InboxError::Undecodable {
                    effect: "e".into(),
                    detail: "d".into(),
                },
                "SINBOX_015",
                422,
                false,
            ),
            (
                InboxError::EffectNotFound { effect: "e".into() },
                "SINBOX_016",
                404,
                false,
            ),
            (
                InboxError::InvalidEventId {
                    effect: "e".into(),
                    detail: "d".into(),
                },
                "SINBOX_006",
                422,
                false,
            ),
            (
                InboxError::EventIdReused { effect: "e".into() },
                "SINBOX_007",
                409,
                false,
            ),
            (
                InboxError::BeyondHorizon {
                    effect: "e".into(),
                    detail: "d".into(),
                },
                "SINBOX_008",
                422,
                false,
            ),
            (
                InboxError::HandlerPanicked { effect: "e".into() },
                "SINBOX_009",
                500,
                false,
            ),
            (
                InboxError::DeadlineExceeded {
                    effect: "e".into(),
                    deadline_secs: 30,
                },
                "SINBOX_010",
                504,
                true,
            ),
            (
                InboxError::InvalidEffectName { name: "N".into() },
                "SINBOX_011",
                500,
                false,
            ),
            (
                InboxError::ParkedNotFound {
                    address: "a".into(),
                },
                "SINBOX_013",
                404,
                false,
            ),
            (
                InboxError::InvalidParkedAddress {
                    address: "a".into(),
                    detail: "d".into(),
                },
                "SINBOX_014",
                400,
                false,
            ),
        ];
        for (error, code, status, retryable) in cases {
            assert_eq!(error.error_code(), code);
            assert_eq!(error.http_status_code(), status, "{code}");
            assert_eq!(error.is_retryable(), retryable, "{code}");
        }
    }

    #[test]
    fn anything_but_a_missing_table_is_the_store_being_unavailable() {
        let error = InboxError::from_store(DatabaseError::internal("connection reset"));
        assert!(matches!(error, InboxError::Unavailable { .. }));
    }
}
