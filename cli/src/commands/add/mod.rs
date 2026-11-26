//! `fun add`: extend a project `fun new` created, located from the nearest `fun.toml`.

mod app;
mod app_lib;
mod authz;
mod command;
mod commands;
mod input;
mod migrations;
mod run;
mod seeds;
mod surface;

pub(crate) use input::AddArgs;
pub(crate) use run::run;

use input::{
    AddAppArgs, AddAuthzArgs, AddCommand, AddCommandArgs, AddCommandsArgs, AddMigrationsArgs,
    AddSeedsArgs, AddSurfaceArgs,
};
