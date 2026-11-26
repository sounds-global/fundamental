//! `fun run`: run a `[commands]` script or a Rust `#[command]` by its address.

mod input;
// The handler is named `run`, like its module.
#[allow(clippy::module_inception)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
mod run;

pub(crate) use input::RunArgs;
pub(crate) use run::run;
