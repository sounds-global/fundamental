//! `fun keys mint`: a new key file, and the configuration lines that read it.

use std::path::Path;

use crate::commands::keys::input::{KeysArgs, KeysCommand};
use crate::commands::keys::mint::mint_key_file;
use crate::{CliResult, CommandContext};

/// Beside the manifest and out of the repository.
const SECRETS: &str = "secrets";

/// Writes the key version under `secrets/` and prints the keyring configuration that reads it.
///
/// # Errors
/// [`CliError::KeyPurposeInvalid`](crate::CliError::KeyPurposeInvalid),
/// [`CliError::KeyExists`](crate::CliError::KeyExists) or
/// [`CliError::KeyUnwritable`](crate::CliError::KeyUnwritable).
pub(crate) fn run(args: &KeysArgs, _ctx: &CommandContext) -> CliResult<()> {
    match &args.command {
        KeysCommand::Mint {
            purpose,
            key_version: version,
        } => {
            let path = mint_key_file(Path::new(SECRETS), purpose, *version)?;
            let file = path.to_string_lossy().replace('\\', "/");
            println!(
                "current = {version}\nversions = [{{ version = {version}, material = \"${{file:{file}}}\" }}]"
            );
            Ok(())
        }
    }
}
