//! `CqlPool` is not `Transactional`, so `begin()` must not resolve.
//!
//! CQL has no multi-statement ACID transactions; use logged batches (atomic) or
//! LWT (single-partition compare-and-set) instead. This file is expected to fail
//! to compile — the paired `no_begin.stderr` snapshot pins the error.

use fundamental_db::Transactional;
use fundamental_cql::CqlPool;

async fn attempts_transaction(pool: &CqlPool) {
    // `CqlPool: !Transactional`, so no `begin` method is in scope.
    let _ = pool.begin().await;
}

fn main() {}
