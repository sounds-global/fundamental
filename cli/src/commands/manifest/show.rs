//! `fun manifest show`: one key's value on stdout, and where it is written.

use clap::Args;
use fundamental_manifest::{KeyPath, Manifest};
use tracing::instrument;

use crate::emit::emit;
use crate::helpers::Project;
use crate::{CliError, CliResult, CommandContext};

#[derive(Debug, Args)]
pub(crate) struct ShowArgs {
    /// The key, as TOML writes it: `local.apexes`, `custom.appendix_a."privacy.backup_days"`
    pub key: String,
}

/// Print the value as TOML, and each file and line it comes from.
///
/// # Errors
/// [`CliError::NotInProject`], [`CliError::Manifest`] for a bad manifest or key, or
/// [`CliError::ManifestKeyAbsent`].
#[instrument(skip_all, fields(key = %args.key), err(Debug))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) fn run(args: &ShowArgs, ctx: &CommandContext) -> CliResult<()> {
    let project = Project::discover()?;
    let manifest = Manifest::load(project.root())?;
    let key: KeyPath = args.key.parse()?;
    let (value, origins) = describe(&manifest, &key)?;
    emit(&value);
    for origin in origins {
        ctx.ui().detail(origin);
    }
    Ok(())
}

fn describe(manifest: &Manifest, key: &KeyPath) -> Result<(String, Vec<String>), CliError> {
    let value = manifest
        .get(key)
        .ok_or_else(|| CliError::ManifestKeyAbsent {
            key: key.to_string(),
        })?;
    let written = match value {
        toml::Value::Table(table) => toml::to_string(table)
            .map_err(|error| CliError::ManifestKeyAbsent {
                key: format!("{key} ({error})"),
            })?
            .trim_end()
            .to_owned(),
        other => other.to_string(),
    };
    let origins = manifest
        .origins(key)
        .iter()
        .map(ToString::to_string)
        .collect();
    Ok((written, origins))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn show_names_file_and_line() -> Result<(), Box<dyn std::error::Error>> {
        let project = tempfile::tempdir()?;
        fs::write(
            project.path().join("fun.toml"),
            "[project]\nname = \"x\"\n\n[custom.appendix_a]\n\"privacy.backup_days\" = 14\n",
        )?;
        fs::write(
            project.path().join("fun.local.toml"),
            "# Appendix A.\n[custom.appendix_a]\n\"privacy.backup_days\" = 30\n",
        )?;
        let manifest = Manifest::load(project.path())?;
        let (value, origins) = describe(
            &manifest,
            &r#"custom.appendix_a."privacy.backup_days""#.parse()?,
        )?;
        assert_eq!(value, "30");
        assert_eq!(origins, ["fun.local.toml:3:1"]);
        let (table, _) = describe(&manifest, &"custom.appendix_a".parse()?)?;
        assert_eq!(table, "\"privacy.backup_days\" = 30");
        let absent = describe(&manifest, &"custom.nothing".parse()?);
        assert!(
            matches!(absent, Err(CliError::ManifestKeyAbsent { .. })),
            "{absent:?}"
        );
        Ok(())
    }
}
