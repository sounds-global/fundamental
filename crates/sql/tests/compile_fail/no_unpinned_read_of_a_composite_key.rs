//! A composite-key model must not be **read** through a query that pins no key column.
//!
//! The other pins in this directory guard a *point* operation — `find`, `delete`, a locked
//! read. This guards the filter surface: a query over `(zone, holder_account_id)` that names
//! neither has no index prefix to seek on, so it would scan every jurisdiction in the table
//! and return whichever rows matched first — plausible rows, and nothing failing. Pinning a
//! leading key column is what gives the query a prefix to seek on.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_core::model::SqlDb;
use fundamental_db::{Filter, QueryBuilder, QueryExecutor};
use fundamental_macros::Model;

#[derive(Clone, Model)]
#[model(sql)]
struct HolderAccount {
    #[model(id)]
    zone: String,
    #[model(id)]
    holder_account_id: String,
    created_at: String,
}

async fn reads_every_jurisdiction(pool: &fundamental_sql::SqlPool) {
    let query = QueryBuilder::<HolderAccount, SqlDb>::new()
        .filter(Filter::eq("holder_account_id", "acct-1"));
    // No `.pin_key(zone)`: the leading key column is unbound, so this cannot be executed.
    let _ = pool.query_executor::<HolderAccount>().execute(&query).await;
}

fn main() {}
