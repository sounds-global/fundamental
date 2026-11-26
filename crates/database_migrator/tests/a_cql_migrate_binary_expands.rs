//! What `migrate_main!` expands to compiles, without a warning, for the backend sets with
//! YCQL: alone, and beside YSQL and the offline store.
//!
//! Compiled and never run. The query depth is raised, as an app's migrate binary with a `cql`
//! backend raises it.

#![cfg(not(target_arch = "wasm32"))]
#![recursion_limit = "256"]
#![deny(warnings)]

use fundamental_db::migration::MigrationGuard;

fn sql_pool() -> fundamental_sql::SqlPool {
    unimplemented!("never run")
}

fn cql_pool() -> fundamental_cql::CqlPool {
    unimplemented!("never run")
}

fn offline_db() -> fundamental_offline::db::OfflineDatabase {
    unimplemented!("never run")
}

mod every_backend {
    use super::{MigrationGuard, cql_pool, offline_db, sql_pool};

    fundamental_macros::migrate_main! {
        app = "probe",
        migrations_dir = "migrations",
        guard = MigrationGuard::unenforced(),
        sql = (sql_pool(), []),
        cql = (cql_pool(), []),
        offline = (offline_db(), []),
    }

    #[test]
    fn expands() {
        let main: fn() -> std::process::ExitCode = main;
        assert!(!format!("{main:p}").is_empty());
    }
}

mod cql_only {
    use super::{MigrationGuard, cql_pool};

    fundamental_macros::migrate_main! {
        app = "probe",
        migrations_dir = "migrations",
        guard = MigrationGuard::unenforced(),
        cql = (cql_pool(), []),
    }

    #[test]
    fn expands() {
        let main: fn() -> std::process::ExitCode = main;
        assert!(!format!("{main:p}").is_empty());
    }
}
