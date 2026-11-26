//! An outbox row must be sealed.
//!
//! A relay carries what a row holds to somewhere that trusts it, so a model without
//! `#[model(outbox)]` — whose rows nothing seals — cannot be drained as an outbox at all.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use std::borrow::Cow;
use chrono::{DateTime, Utc};
use fundamental_core::model::{HasLeadingKey, SqlDb};
use fundamental_db::{OutboxRow, QueryBuilder};
use fundamental_macros::Model;

#[derive(Clone, Debug, Model)]
#[model(sql)]
struct Unsealed {
    #[model(id)]
    id: uuid::Uuid,
    next_attempt_at: DateTime<Utc>,
}

impl OutboxRow<SqlDb> for Unsealed {
    type Scope = ();

    fn msg_id(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn attempts(&self) -> i64 {
        0
    }

    fn pending(_scope: &Self::Scope) -> QueryBuilder<Self, SqlDb, HasLeadingKey> {
        QueryBuilder::new().already_addressed()
    }

    fn next_attempt_column() -> &'static str {
        "next_attempt_at"
    }

    fn defer_until(&mut self, at: DateTime<Utc>) {
        self.next_attempt_at = at;
    }

    fn note_failure(&mut self, _error: &str) {}
}

fn main() {}
