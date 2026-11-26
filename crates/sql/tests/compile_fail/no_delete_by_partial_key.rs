//! A composite-key model must not be **deletable** by one of its key columns.
//!
//! The read-side sibling is `no_find_by_partial_key.rs`. This one is the sharper case: a
//! prefix-less `find` returns the wrong row, which a caller may still notice, while a
//! prefix-less `DELETE … WHERE zone = $1` removes every row in a jurisdiction and there is
//! nothing left to notice it with.
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

async fn deletes_by_half_a_key(pool: &fundamental_sql::SqlPool) {
    // `zone` alone is a prefix, not a key. The `M::Id: Encode` bound on `delete_by_id` is
    // what refuses this — a composite-key `Id` is a marker and deliberately not `Encode`.
    let _ =
        fundamental_sql::SqlModelQuery::<HolderAccount>::delete_by_id(pool, &"eu")
            .await;
}

fn main() {}
