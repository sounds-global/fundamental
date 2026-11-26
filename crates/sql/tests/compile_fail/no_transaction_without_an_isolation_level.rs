//! A transaction must not be openable without saying what isolation it runs at.
//!
//! A defaulting `begin()` or `transaction(f)` would put the guarantee a read-modify-write
//! depends on into the deployed engine's configuration rather than into the code.
//! PostgreSQL defaults to `READ COMMITTED`, and YSQL's default depends on the cluster's
//! `yb_enable_read_committed_isolation` flag, so the same source would be a different
//! promise depending on where it ran, and nothing would say so.
//!
//! There is no defaulting form. This pins that, because a re-added convenience overload
//! would be the easiest possible regression to justify and the hardest to notice.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_db::DatabaseError;

async fn opens_without_deciding(pool: &fundamental_sql::SqlPool) {
    let _: Result<(), DatabaseError> = pool.transaction(async |_tx| Ok(())).await;
}

fn main() {}
