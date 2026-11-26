//! The [`CliResult`] alias for results returned by CLI command handlers.

use crate::CliError;

pub type CliResult<T> = Result<T, CliError>;
