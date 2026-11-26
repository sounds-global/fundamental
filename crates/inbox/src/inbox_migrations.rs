//! [`INBOX_MIGRATIONS`] — the inbox's tables, for an app's migrator.

use fundamental_db::migration::EmbeddedMigrations;

use crate::effect_row::EffectRow;
use crate::inbox_row::InboxRow;
use crate::parked_row::ParkedRow;

/// The inbox tables, for `migrate_main!(.., embedded = [&INBOX_MIGRATIONS])`.
pub static INBOX_MIGRATIONS: EmbeddedMigrations = fundamental_macros::migrations!(
    owner = "fundamental_inbox",
    models = [InboxRow, ParkedRow, EffectRow]
);
