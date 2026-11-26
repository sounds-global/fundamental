//! `fun migrate`: manage an app's database migrations.

mod create;
mod generate;
mod input;
mod list;
mod rebase;
mod resolve;
mod revision;
mod run;
mod spawn;
mod timestamp;
mod verify;

pub(crate) use input::MigrateArgs;
pub(crate) use run::run;
pub(crate) use timestamp::migration_timestamp;

use input::{
    MigrateCommand, MigrateCreateArgs, MigrateListArgs, MigrateRebaseArgs, MigrateResolveArgs,
    MigrateVerifyArgs,
};
