//! `fun authz schema-validate`: validate a schema definition file.

use std::fs;

use clap::Args;
use fundamental::authz::schema::SchemaLoader;

use crate::{CliError, CliResult, CommandContext};

#[derive(Debug, Args)]
pub(crate) struct SchemaValidateArgs {
    /// Path to a JSON schema definition file
    pub path: String,
}

/// Compile the file through the engine's validation; an invalid schema forwards its `AUTHZ_0xx` code.
pub(crate) fn run(args: SchemaValidateArgs, ctx: CommandContext) -> CliResult<()> {
    let json = fs::read_to_string(&args.path).map_err(|err| CliError::AuthzSchemaRead {
        path: args.path.clone(),
        reason: err.to_string(),
    })?;

    let schema = SchemaLoader::from_json(&json)?;

    let ui = ctx.ui();
    ui.section("Schema valid");
    ui.detail(format!(
        "{} namespace(s), schema hash {}",
        schema.len(),
        schema.hash()
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Probe {
        #[command(flatten)]
        args: SchemaValidateArgs,
    }

    #[test]
    fn takes_the_schema_path_positionally() {
        let probe = Probe::try_parse_from(["fun", "authz.schema.json"]).expect("a path is valid");
        assert_eq!(probe.args.path, "authz.schema.json");
    }

    #[test]
    fn requires_a_path() {
        assert!(Probe::try_parse_from(["fun"]).is_err());
    }
}
