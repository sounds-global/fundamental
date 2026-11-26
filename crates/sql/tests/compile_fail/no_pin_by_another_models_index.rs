//! An index declared on one model cannot pin a query over another.
//!
//! `IndexKey<M>` is typed by its model. Without that, `memberships_by_account` could be handed to a
//! query over any table, and the query would reach the addressed state while binding a column the
//! table may not even have — or one no index of *that* table leads with, which is a scan.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_core::model::SqlDb;
use fundamental_db::QueryBuilder;
use fundamental_macros::Model;

#[derive(Clone, Model)]
#[model(
    sql,
    index(name = "memberships_by_account", columns = ["staff_account_id", "organisation_id"])
)]
struct Membership {
    #[model(id)]
    organisation_id: String,
    #[model(id)]
    staff_account_id: String,
}

#[derive(Clone, Model)]
#[model(sql)]
struct HolderAccount {
    #[model(id)]
    zone: String,
    #[model(id)]
    holder_account_id: String,
}

fn main() {
    let _ = QueryBuilder::<HolderAccount, SqlDb>::new()
        .pin_index(Membership::MEMBERSHIPS_BY_ACCOUNT, "acct-1");
}
