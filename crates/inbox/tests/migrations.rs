//! The inbox's models are its migrations: a model changed without its migration fails here, and
//! `fun migrate generate <name>` writes the file it needs.

#[test]
fn the_models_are_their_migrations() {
    fundamental_db::migration::generate::the_models_are_their_migrations(
        &fundamental_inbox::INBOX_MIGRATIONS,
    );
}
