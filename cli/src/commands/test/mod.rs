//! `fun test`: the workspace's tests through nextest's `agent` profile, failures read from JUnit.

mod changed_files;
mod local_package;
mod reads_cover;
mod run;
mod selection;
mod test_args;
mod test_failure;
mod workspace;

pub(crate) use run::{metadata, run};
pub(crate) use test_args::TestArgs;
