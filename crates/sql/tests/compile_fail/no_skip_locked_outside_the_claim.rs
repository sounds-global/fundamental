//! `SKIP LOCKED` must not be nameable outside this crate.
//!
//! The clause is only honoured at some isolation levels, and the engines that do not honour
//! it do not reliably fail: YugabyteDB's YSQL emits a warning once per row and then ignores
//! it under `SERIALIZABLE`, so the statement succeeds, rows come back, and a claim has
//! quietly become a plain `FOR UPDATE` — two drains blocking each other instead of taking
//! disjoint work. Clients discard warnings, so nothing is logged and no test that checks the
//! rows would notice.
//!
//! That pairing cannot be made safe by documenting it, so the clause and the isolation it
//! depends on live together in this crate's outbox claim, and downstream code claims work
//! through `OutboxClaimer`. This pins the absence, because an absence rots silently.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_core::model::SqlDb;
use fundamental_db::QueryBuilder;
use fundamental_macros::Model;

#[derive(Clone, Model)]
#[model(sql, table = "jobs")]
struct Job {
    #[model(id)]
    id: uuid::Uuid,
}

fn claims_outside_the_framework() {
    let _ = fundamental_sql::SqlQuery::new(QueryBuilder::<Job, SqlDb>::new())
        .for_update_skip_locked();
}

fn main() {}
