//! A composite-key model must not be fetchable by one of its key columns.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_macros::Model;

#[derive(Model)]
#[model(sql)]
struct HolderAccount {
    #[model(id)]
    zone: String,
    #[model(id)]
    holder_account_id: String,
    created_at: String,
}

async fn fetches_by_half_a_key(pool: &fundamental_sql::SqlPool) {
    // `zone` alone is a prefix, not a key. There is no `find` on a composite-key model.
    let _ = HolderAccount::find(pool, &"eu".to_string()).await;
}

fn main() {}
