//! The lease's models are its migrations.

#[test]
fn the_models_are_their_migrations() {
    fundamental_db::migration::generate::the_models_are_their_migrations(
        &fundamental_lease::LEASE_MIGRATIONS,
    );
}
