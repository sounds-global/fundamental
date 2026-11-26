//! A transaction body that mutates the caller's frame must not open a transaction.
//!
//! `SERIALIZABLE` makes running the body again the client's job, so the body has to be
//! re-runnable. `Clone` is that property, checked by the compiler: a closure over shared
//! references is `Clone` for free, and one that captures `&mut` state is not — because a
//! second attempt would run over its own half-applied mutation, which a rollback undoes
//! inside the transaction and never outside it.
//!
//! Pinned by an absence, beside `no_transaction_without_an_isolation_level`, for the same
//! reason: an absence rots silently. The fix at a real call site is to build the value
//! before the closure and move it in.

use fundamental_db::{DatabaseError, IsolationLevel};

async fn retries_over_its_own_mutation(pool: &fundamental_sql::SqlPool) {
    let mut minted: Vec<i64> = Vec::new();
    let _: Result<(), DatabaseError> = pool
        .transaction(IsolationLevel::Serializable, async |_tx| {
            minted.push(1);
            Ok(())
        })
        .await;
}

fn main() {}
