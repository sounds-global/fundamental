//! Arguments of the `fun generate` family.

use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub(crate) struct GenerateArgs {
    #[command(subcommand)]
    pub command: GenerateCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum GenerateCommand {
    /// Scaffold a domain model and its migration.
    Model(GenerateModelArgs),

    /// Scaffold a domain action (an HTTP handler) and its request/response types.
    Action(GenerateActionArgs),

    /// Scaffold a domain slice (an offline-sync query) and its syncable base model.
    Slice(GenerateSliceArgs),
}

#[derive(Debug, Args)]
pub(crate) struct GenerateSliceArgs {
    /// The slice name, a Rust identifier such as `mine`; its PascalCase names the marker and model
    pub name: String,

    /// The backend the example model targets, `sql` or `cql` (default: from `fun.toml`)
    #[arg(long)]
    pub db: Option<String>,

    /// The domain crate to scaffold into, when the project has several
    #[arg(long = "crate")]
    pub crate_name: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct GenerateActionArgs {
    /// The action name, a Rust identifier such as `greet`; its PascalCase prefixes the types
    pub name: String,

    /// The domain crate to scaffold into, when the project has several
    #[arg(long = "crate")]
    pub crate_name: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct GenerateModelArgs {
    /// The model name in PascalCase (a valid Rust type identifier), e.g. `BlogPost`.
    pub name: String,

    /// A field, as `name:type`, repeatable: `-f title:String -f views:i64`
    #[arg(short = 'f', long = "field")]
    pub fields: Vec<String>,

    /// The app the migration belongs to (the model goes into a domain crate)
    #[arg(long, default_value = "main")]
    pub app: String,

    /// The database backend to target: `sql` or `cql`.
    #[arg(long, default_value = "sql")]
    pub db: String,

    /// The domain crate to scaffold into, when the project has several
    #[arg(long = "crate")]
    pub crate_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Probe {
        #[command(flatten)]
        args: GenerateModelArgs,
    }

    #[test]
    fn defaults_to_main_and_sql_with_no_fields() {
        let probe = Probe::try_parse_from(["fun", "Post"]).expect("name only is valid");
        assert_eq!(probe.args.name, "Post");
        assert_eq!(probe.args.app, "main");
        assert_eq!(probe.args.db, "sql");
        assert!(probe.args.fields.is_empty());
    }

    #[test]
    fn collects_repeated_field_flags() {
        let probe = Probe::try_parse_from([
            "fun",
            "Post",
            "-f",
            "title:String",
            "-f",
            "views:i64",
            "--db",
            "cql",
        ])
        .expect("flags are valid");
        assert_eq!(probe.args.fields, vec!["title:String", "views:i64"]);
        assert_eq!(probe.args.db, "cql");
    }
}
