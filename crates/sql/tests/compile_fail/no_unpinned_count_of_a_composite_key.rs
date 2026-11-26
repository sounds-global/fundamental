//! A composite-key model must not offer a whole-table `count`, nor an unpinned `update_where`.
//!
//! Both were emitted by `#[derive(Model)]` for **every** model, including composite-key ones,
//! and both were missed when `find`/`update`/`delete` were suppressed. `count(conn)` is
//! `SELECT COUNT(*)` with no predicate at all; `update_where(conn)` is an `UPDATE` whose
//! predicate the caller supplies and nothing checks. On a key that leads with a jurisdiction
//! the first counts all of them and the second can write to all of them.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

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

async fn counts_every_jurisdiction(pool: &fundamental_sql::SqlPool) {
    let _ = HolderAccount::count(pool).await;
}

fn updates_every_jurisdiction(pool: &fundamental_sql::SqlPool) {
    let _ = HolderAccount::update_where(pool);
}

fn main() {}
