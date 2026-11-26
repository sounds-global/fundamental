//! A composite-key row must not be lockable by one of its key columns.
//!
//! The locked read is where this matters most: `find_for_update` is the first half of every
//! read-decide-write sequence, so a lock taken on a prefix would serialize writers on the
//! wrong set of rows — two transactions each believing they hold the row they are about to
//! write, in different tenancies.
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

async fn locks_half_a_key(tx: &mut fundamental_sql::SqlTransaction) {
    // `find_for_update` requires `M::Id: sqlx::Encode`, which a composite `Id` deliberately
    // is not. `find_for_update_by_key` is the method that exists for this model.
    let _ = tx.find_for_update::<HolderAccount>(&"eu".to_string()).await;
}

fn main() {}
