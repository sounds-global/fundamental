//! Shared helpers for command handlers.

mod app_bin;
mod cargo_target_dir;
mod command_entry;
mod command_index;
mod command_source;
mod crate_manifest;
mod domain_crate;
mod git;
mod main_bin;
mod manifest;
mod manifest_command;
mod migration_stub;
mod module_decl;
mod project;
mod workspace_members;
mod write_binary_file;
mod write_file;

pub(crate) use app_bin::{bin_names, resolve_app_bin};
pub(crate) use cargo_target_dir::cargo_target_dir;
pub(crate) use command_entry::CommandEntry;
pub(crate) use command_index::CommandIndex;
pub(crate) use command_source::CommandSource;
pub(crate) use crate_manifest::CrateManifest;
pub(crate) use domain_crate::DomainCrate;
pub(crate) use git::git;
pub(crate) use main_bin::resolve_main_bin;
pub(crate) use manifest::Manifest;
pub(crate) use manifest_command::ManifestCommand;
pub(crate) use migration_stub::{down_stub, up_stub};
pub(crate) use module_decl::ensure_module_decl;
pub(crate) use project::Project;
pub(crate) use workspace_members::add_workspace_member;
pub(crate) use write_binary_file::write_binary_file;
pub(crate) use write_file::write_file;
