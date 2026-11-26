//! [`ParkReason`] — why the inbox refused to apply a delivery.

use std::fmt;
use std::str::FromStr;

use fundamental_core::Error as _;

use crate::InboxError;

/// Why a delivery was parked rather than applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParkReason {
    /// The payload does not decode as the handler's message.
    Undecodable,
    /// The event id is not a UUIDv7, or the `Nats-Msg-Id` is missing or names another id.
    InvalidEventId,
    /// An applied event id arrived with different content.
    EventIdReused,
    /// The event is older than the retention window allows.
    BeyondHorizon,
    /// The event's timestamp is further ahead of the database's clock than skew explains.
    FromTheFuture,
    /// Every attempt the budget allows failed.
    AttemptsExhausted,
    /// The handler panicked on the last attempt the budget allows.
    HandlerPanicked,
}

impl ParkReason {
    /// Every reason, in the order the table's constraint lists them.
    pub const ALL: [Self; 7] = [
        Self::Undecodable,
        Self::InvalidEventId,
        Self::EventIdReused,
        Self::BeyondHorizon,
        Self::FromTheFuture,
        Self::AttemptsExhausted,
        Self::HandlerPanicked,
    ];

    /// The error that refuses a delivery for this reason, for `effect`, or `None` for
    /// [`AttemptsExhausted`](Self::AttemptsExhausted), whose code is the last failure's own.
    #[must_use]
    pub fn error(&self, effect: &str, detail: impl Into<String>) -> Option<InboxError> {
        let effect = effect.to_owned();
        let detail = detail.into();
        Some(match self {
            Self::Undecodable => InboxError::Undecodable { effect, detail },
            Self::InvalidEventId => InboxError::InvalidEventId { effect, detail },
            Self::EventIdReused => InboxError::EventIdReused { effect },
            Self::BeyondHorizon | Self::FromTheFuture => {
                InboxError::BeyondHorizon { effect, detail }
            }
            Self::HandlerPanicked => InboxError::HandlerPanicked { effect },
            Self::AttemptsExhausted => return None,
        })
    }

    /// The error code a record parked for this reason carries, where the reason has its own.
    #[must_use]
    pub fn code(&self) -> Option<&'static str> {
        self.error("", "").map(|error| error.error_code())
    }

    /// The value stored in the `reason` column.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Undecodable => "undecodable",
            Self::InvalidEventId => "invalid_event_id",
            Self::EventIdReused => "event_id_reused",
            Self::BeyondHorizon => "beyond_horizon",
            Self::FromTheFuture => "from_the_future",
            Self::AttemptsExhausted => "attempts_exhausted",
            Self::HandlerPanicked => "handler_panicked",
        }
    }
}

impl fmt::Display for ParkReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ParkReason {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|reason| reason.as_str() == value)
            .ok_or_else(|| format!("`{value}` is not a park reason"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reason_round_trips_through_its_column_value() {
        for reason in ParkReason::ALL {
            assert_eq!(reason.as_str().parse::<ParkReason>(), Ok(reason));
        }
        assert!("lost".parse::<ParkReason>().is_err());
    }

    #[test]
    fn a_reason_carries_its_error_code_except_exhaustion() {
        assert_eq!(ParkReason::Undecodable.code(), Some("SINBOX_015"));
        assert_eq!(ParkReason::InvalidEventId.code(), Some("SINBOX_006"));
        assert_eq!(ParkReason::EventIdReused.code(), Some("SINBOX_007"));
        assert_eq!(ParkReason::BeyondHorizon.code(), Some("SINBOX_008"));
        assert_eq!(ParkReason::FromTheFuture.code(), Some("SINBOX_008"));
        assert_eq!(ParkReason::HandlerPanicked.code(), Some("SINBOX_009"));
        assert_eq!(ParkReason::AttemptsExhausted.code(), None);
    }

    #[test]
    fn the_table_constraint_admits_exactly_these_reasons() {
        let ddl = include_str!("../migrations/sql/0002_fundamental_inbox_parked.up.sql");
        let admitted = ddl.matches('\'').count() / 2;
        assert_eq!(admitted, ParkReason::ALL.len());
        for reason in ParkReason::ALL {
            assert!(
                ddl.contains(&format!("'{}'", reason.as_str())),
                "{reason} is not admitted"
            );
        }
    }
}
