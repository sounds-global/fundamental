//! [`rehome_command`] — `fun run inbox:rehome`, handing an effect to a renamed handler.

use std::io::Write as _;

use fundamental_command::{Command, CommandArgs, CommandError};
use fundamental_core::Error as _;

use crate::InboxContext;

/// The `inbox:rehome` command: `inbox:rehome <effect> <handler>` hands the effect's ledger to
/// the handler at that `module::path::name`, when the handler that applied it was renamed or
/// moved and its listener is refused with `SINBOX_005`.
pub fn rehome_command<C: InboxContext + Sync>() -> Command<C> {
    Command::new(
        "inbox",
        "rehome",
        Some("Hand an effect to its renamed handler: `<effect> <module::path::handler>`"),
        |ctx, args| Box::pin(rehome(ctx, args)),
    )
}

async fn rehome<C: InboxContext + Sync>(ctx: &C, args: CommandArgs) -> Result<(), CommandError> {
    let (Some(effect), Some(handler)) = (args.get(0), args.get(1)) else {
        return Err(CommandError::msg(
            "the effect and the handler it moves to are required: `<effect> <module::path::handler>`",
        ));
    };
    ctx.inbox()
        .rehome(effect, handler)
        .await
        .map_err(|error| CommandError::msg(format!("{} ({})", error, error.error_code())))?;
    writeln!(
        std::io::stdout().lock(),
        "{effect} is applied by {handler}."
    )
    .map_err(|error| CommandError::msg(format!("writing: {error}")))
}
