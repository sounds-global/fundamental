//! `fun migrate resolve`: settle a failed migration through the app's `migrate` binary.

use super::{MigrateResolveArgs, spawn};
use crate::{CliResult, CommandContext};

/// Runs the app's migrate binary with `resolve` and the act.
///
/// # Errors
/// As [`spawn::run`].
pub(super) fn run(args: &MigrateResolveArgs, ctx: &CommandContext) -> CliResult<()> {
    spawn::run(args.app.as_deref(), args.bin.as_deref(), &words(args), ctx)
}

fn words(args: &MigrateResolveArgs) -> [&str; 5] {
    let act = if args.applied { "--applied" } else { "--retry" };
    ["resolve", &args.name, act, "--reason", &args.reason]
}

#[cfg(test)]
mod tests {
    use super::{MigrateResolveArgs, words};

    fn args(retry: bool) -> MigrateResolveArgs {
        MigrateResolveArgs {
            name: "0007_index_orders".to_owned(),
            retry,
            applied: !retry,
            reason: "timeout at peak; rerun at night".to_owned(),
            app: None,
            bin: None,
        }
    }

    #[test]
    fn the_binary_is_asked_the_act_and_the_reason_as_one_word_each() {
        assert_eq!(
            words(&args(true)),
            [
                "resolve",
                "0007_index_orders",
                "--retry",
                "--reason",
                "timeout at peak; rerun at night"
            ]
        );
        assert_eq!(words(&args(false))[2], "--applied");
    }
}
