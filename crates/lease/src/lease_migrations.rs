use fundamental_db::migration::EmbeddedMigrations;

use crate::{LeaseRow, LeaseWorkRow};

/// The lease tables, registered with `MigrateRunner::with_embedded(&LEASE_MIGRATIONS)`.
pub static LEASE_MIGRATIONS: EmbeddedMigrations = fundamental_macros::migrations!(
    owner = "fundamental_lease",
    models = [LeaseRow, LeaseWorkRow]
);
