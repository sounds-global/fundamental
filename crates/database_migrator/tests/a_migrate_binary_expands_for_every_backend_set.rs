//! What `migrate_main!` expands to compiles, without a warning and at the compiler's default
//! query depth, for each set of backends an app can declare without YCQL. Compiled and never run.

#![cfg(not(target_arch = "wasm32"))]
#![deny(warnings)]

use fundamental_db::migration::MigrationGuard;

fn sql_pool() -> fundamental_sql::SqlPool {
    unimplemented!("never run")
}

fn offline_db() -> fundamental_offline::db::OfflineDatabase {
    unimplemented!("never run")
}

mod sql_and_offline {
    use super::{MigrationGuard, offline_db, sql_pool};

    fundamental_macros::migrate_main! {
        app = "probe",
        migrations_dir = "migrations",
        guard = MigrationGuard::unenforced(),
        sql = (sql_pool(), []),
        offline = (offline_db(), []),
    }

    #[test]
    fn expands() {
        let main: fn() -> std::process::ExitCode = main;
        assert!(!format!("{main:p}").is_empty());
    }
}

mod sql_only {
    use super::{MigrationGuard, sql_pool};

    fundamental_macros::migrate_main! {
        app = "probe",
        migrations_dir = "migrations",
        guard = MigrationGuard::unenforced(),
        sql = (sql_pool(), []),
    }

    #[test]
    fn expands() {
        let main: fn() -> std::process::ExitCode = main;
        assert!(!format!("{main:p}").is_empty());
    }
}

mod offline_only {
    use super::{MigrationGuard, offline_db};

    fundamental_macros::migrate_main! {
        app = "probe",
        migrations_dir = "migrations",
        guard = MigrationGuard::unenforced(),
        offline = (offline_db(), []),
    }

    #[test]
    fn expands() {
        let main: fn() -> std::process::ExitCode = main;
        assert!(!format!("{main:p}").is_empty());
    }
}
