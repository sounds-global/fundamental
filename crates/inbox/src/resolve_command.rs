//! [`resolve_command`] — `fun run inbox:resolve`, closing one parked record.

use std::io::Write as _;

use fundamental_command::{Command, CommandArgs, CommandError};
use fundamental_core::Error as _;

use crate::{InboxContext, ParkedAddress};

/// The `inbox:resolve` command: `inbox:resolve <address>` removes the parked record at the
/// address `inbox:parked` printed, once the operator has acted on it.
pub fn resolve_command<C: InboxContext + Sync>() -> Command<C> {
    Command::new(
        "inbox",
        "resolve",
        Some(
            "Close a parked delivery once it is handled: `<effect>/<stream>/<created>/<sequence>`",
        ),
        |ctx, args| Box::pin(resolve(ctx, args)),
    )
}

async fn resolve<C: InboxContext + Sync>(ctx: &C, args: CommandArgs) -> Result<(), CommandError> {
    let text = args
        .first()
        .ok_or_else(|| CommandError::msg("the parked record's address is required"))?;
    let refuse =
        |error: crate::InboxError| CommandError::msg(format!("{} ({})", error, error.error_code()));
    let address: ParkedAddress = text.parse().map_err(refuse)?;
    ctx.inbox().resolve(&address).await.map_err(refuse)?;
    writeln!(std::io::stdout().lock(), "Resolved {address}.")
        .map_err(|error| CommandError::msg(format!("writing: {error}")))
}
