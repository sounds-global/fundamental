use fundamental_db::migration::EmbeddedMigrations;

use crate::ScheduledJob;

/// The job table's migrations, for `MigrateRunner::with_embedded(&JOBS_MIGRATIONS)`.
pub static JOBS_MIGRATIONS: EmbeddedMigrations =
    fundamental_macros::migrations!(owner = "fundamental_jobs", models = [ScheduledJob]);
