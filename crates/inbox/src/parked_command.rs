//! [`parked_command`] — `fun run inbox:parked`, the parked records of one effect.

use std::io::Write as _;

use fundamental_command::{Command, CommandArgs, CommandError};
use fundamental_core::Error as _;

use crate::{InboxContext, PARKED_PAGE};

/// The `inbox:parked` command: `--effect <name>` lists that effect's oldest parked records,
/// one per line — the address `inbox:resolve` takes, why it was parked, the failing code, the
/// attempt, when, and the event's id.
pub fn parked_command<C: InboxContext + Sync>() -> Command<C> {
    Command::new(
        "inbox",
        "parked",
        Some("List the deliveries an inbox effect refused: `--effect <name>`"),
        |ctx, args| Box::pin(parked(ctx, args)),
    )
}

async fn parked<C: InboxContext + Sync>(ctx: &C, args: CommandArgs) -> Result<(), CommandError> {
    let effect = args
        .flag_value("--effect")
        .ok_or_else(|| CommandError::msg("`--effect <name>` is required"))?;
    let records = ctx
        .inbox()
        .parked(effect)
        .await
        .map_err(|error| CommandError::msg(format!("{} ({})", error, error.error_code())))?;
    let mut out = std::io::stdout().lock();
    let write = |out: &mut std::io::StdoutLock<'_>, line: String| {
        writeln!(out, "{line}").map_err(|error| CommandError::msg(format!("writing: {error}")))
    };
    if records.is_empty() {
        return write(&mut out, format!("Nothing is parked for {effect}."));
    }
    for record in &records {
        write(
            &mut out,
            format!(
                "{}\t{}\t{}\tattempt {}\t{}\t{}",
                record.address,
                record.reason.map_or("unknown", |reason| reason.as_str()),
                record.error_code.as_deref().unwrap_or("-"),
                record.delivered,
                record.parked_at.to_rfc3339(),
                record
                    .event_id
                    .map_or_else(|| "-".to_owned(), |id| id.to_string()),
            ),
        )?;
    }
    if u64::try_from(records.len()).is_ok_and(|shown| shown == PARKED_PAGE) {
        write(
            &mut out,
            format!("The oldest {PARKED_PAGE} are shown; resolve them to see the rest."),
        )?;
    }
    Ok(())
}
