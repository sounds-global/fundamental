//! `fun add command`: add a `[commands.<namespace>]` shell script to `fun.toml`.

use fundamental::CommandAddress;
use tracing::instrument;

use super::AddCommandArgs;
use crate::helpers::{Manifest, Project};
use crate::{CliError, CliResult, CommandContext};

/// Write a `[commands.<namespace>].<name>` entry into `fun.toml`, preserving its formatting.
///
/// # Errors
/// [`CliError::ManifestCommandShape`] for an address not in kebab-case `<namespace>:<name>`, or a
/// [`CliError`] for a missing project or unreadable manifest.
#[instrument(skip(ctx), fields(name = %args.name), err(Debug))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) fn run(args: AddCommandArgs, ctx: CommandContext) -> CliResult<()> {
    let project = Project::discover()?;
    let mut manifest = Manifest::load(&project.manifest_path())?;

    // Refused here rather than on the next `fun run`.
    let address =
        CommandAddress::parse(&args.name).map_err(|objection| CliError::ManifestCommandShape {
            path: project.manifest_path().display().to_string(),
            namespace: args.name.clone(),
            name: args.name.clone(),
            reason: format!("is not an address: it {objection}"),
        })?;

    let line = args.run.join(" ");
    manifest.set_command(address.namespace(), address.name(), &line)?;
    manifest.save()?;

    let ui = ctx.ui();
    ui.blank();
    ui.section(format!("Added command {address}"));
    ui.detail(&line);
    ui.blank();
    ui.section("Next steps");
    ui.command(format!("fun run {address}"));
    ui.blank();
    Ok(())
}
