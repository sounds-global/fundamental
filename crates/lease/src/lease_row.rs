use chrono::{DateTime, Utc};
use fundamental_macros::Model;
use uuid::Uuid;

use crate::{LeaseError, LeaseState};

/// A lease as its table stores it — one row per singleton, keyed by the lease's name.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(
    sql,
    table = "fundamental_leases",
    check(name = "fundamental_leases_epoch_positive", expr = r#"epoch >= 1"#),
    check(
        name = "fundamental_leases_held_is_whole",
        expr = r#"(holder IS NULL AND acquired_at IS NULL AND expires_at IS NULL) OR (holder IS NOT NULL AND acquired_at IS NOT NULL AND expires_at IS NOT NULL AND acquired_at <= changed_at AND changed_at < expires_at)"#
    ),
    check(
        name = "fundamental_leases_name_shape",
        expr = r#"name ~ '^[a-z][a-z0-9-]{0,62}$'"#
    )
)]
pub struct LeaseRow {
    /// The lease's name, its key.
    #[model(id)]
    name: String,
    /// The fencing epoch of the current or last tenure.
    epoch: i64,
    /// The holding process, while held.
    holder: Option<Uuid>,
    /// When the tenure began, while held.
    acquired_at: Option<DateTime<Utc>>,
    /// When another instance may take it, while held.
    expires_at: Option<DateTime<Utc>>,
    /// When the row was last written.
    changed_at: DateTime<Utc>,
}

impl LeaseRow {
    /// The row that records `state` for the lease `name`.
    pub(crate) fn recording(name: &str, state: &LeaseState) -> Self {
        match *state {
            LeaseState::Vacant { epoch, changed_at } => Self {
                name: name.to_owned(),
                epoch,
                holder: None,
                acquired_at: None,
                expires_at: None,
                changed_at,
            },
            LeaseState::Held {
                epoch,
                holder,
                acquired_at,
                expires_at,
                changed_at,
            } => Self {
                name: name.to_owned(),
                epoch,
                holder: Some(holder),
                acquired_at: Some(acquired_at),
                expires_at: Some(expires_at),
                changed_at,
            },
        }
    }

    /// The lease's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the row says about its lease.
    ///
    /// # Errors
    ///
    /// [`LeaseError::CorruptRow`] if the held columns are partly set.
    pub fn state(&self) -> Result<LeaseState, LeaseError> {
        match (self.holder, self.acquired_at, self.expires_at) {
            (None, None, None) => Ok(LeaseState::Vacant {
                epoch: self.epoch,
                changed_at: self.changed_at,
            }),
            (Some(holder), Some(acquired_at), Some(expires_at)) => Ok(LeaseState::Held {
                epoch: self.epoch,
                holder,
                acquired_at,
                expires_at,
                changed_at: self.changed_at,
            }),
            _ => Err(LeaseError::CorruptRow {
                lease: self.name.clone(),
                detail: "holder, acquired_at and expires_at are only partly set".to_owned(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn a_half_held_row_is_refused_as_corrupt() {
        let now = DateTime::<Utc>::UNIX_EPOCH;
        let mut row = LeaseRow::recording(
            "keeper",
            &LeaseState::Vacant {
                epoch: 3,
                changed_at: now,
            },
        );
        assert!(matches!(
            row.state(),
            Ok(LeaseState::Vacant { epoch: 3, .. })
        ));
        row.holder = Some(Uuid::nil());
        assert_eq!(row.state().expect_err("corrupt").error_code(), "SLEASE_006");
    }
}
