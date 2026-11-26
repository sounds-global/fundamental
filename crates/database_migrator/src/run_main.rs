//! [`run_main`] — everything a migrate binary does after its models and connections are known.

use std::process::ExitCode;

use fundamental_core::Error as _;
use fundamental_db::migration::MigrationError;
use tracing::{error, info};

use crate::MigrateCommand;

/// The exit status for a command line that names no subcommand this binary has.
const USAGE: u8 = 2;

/// The line a command line this binary cannot run is answered with.
const USAGE_LINE: &str = "usage: migrate <apply | status | check | verify <base> | resolve <name> \
                          (--retry | --applied) --reason <text>>";

/// Runs `run` for the subcommand `args` — the words after the binary's name — ask for, and
/// turns its outcome into an exit status.
///
/// An unknown or malformed subcommand prints the usage and exits `2`, never `0`; a failure is
/// logged once with its code, printed as `migrate failed [<code>]: …`, and exits `1`.
pub fn run_main<S: AsRef<str>>(
    args: &[S],
    run: impl FnOnce(MigrateCommand) -> Result<(), MigrationError>,
) -> ExitCode {
    let Some(command) = MigrateCommand::parse(args) else {
        let given: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
        eprintln!("{USAGE_LINE}; got {:?}", given.join(" "));
        return ExitCode::from(USAGE);
    };
    let word = command.word();
    let (exit, report) = outcome(word, run(command));
    if let Some(report) = report {
        eprintln!("{report}");
    }
    exit
}

/// The exit status for `result`, and the line a failure prints.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn outcome(command: &str, result: Result<(), MigrationError>) -> (ExitCode, Option<String>) {
    match result {
        Ok(()) => {
            info!(command, "migrate finished");
            (ExitCode::SUCCESS, None)
        }
        Err(failure) => {
            let code = failure.error_code();
            error!(command, code, error = %failure, "migrate failed");
            (
                ExitCode::FAILURE,
                Some(format!("migrate failed [{code}]: {failure}")),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process::ExitCode;

    use fundamental_core::AnyError;
    use fundamental_db::migration::MigrationError;
    use fundamental_macros::Error;

    use super::{MigrateCommand, outcome, run_main};

    #[derive(Debug, Error)]
    enum Provisioning {
        #[error(
            code = "TEST_MIG_001",
            message = "the stream refused its configuration",
            status = 503
        )]
        StreamRefused,
    }

    #[test]
    fn a_failure_raised_in_a_connection_expression_keeps_its_own_code() {
        // What `?` does to an `AnyError` inside a connection expression.
        let failure: MigrationError = AnyError::new(Provisioning::StreamRefused).into();
        let (exit, report) = outcome("apply", Err(failure));
        assert_eq!(exit, ExitCode::FAILURE);
        let report = report.expect("a failure is reported");
        assert!(
            report.starts_with("migrate failed [TEST_MIG_001]: "),
            "{report}"
        );
    }

    #[test]
    #[tracing_test::traced_test]
    fn a_failing_command_exits_non_zero_and_is_logged_once_with_its_code() {
        let exit = run_main(&["apply"], |_| {
            Err(AnyError::new(Provisioning::StreamRefused).into())
        });

        assert_eq!(exit, ExitCode::FAILURE);
        assert!(logs_contain("migrate failed"));
        assert!(logs_contain("TEST_MIG_001"));
        logs_assert(|lines| {
            match lines
                .iter()
                .filter(|line| line.contains("migrate failed"))
                .count()
            {
                1 => Ok(()),
                seen => Err(format!("the failure is logged once, not {seen} times")),
            }
        });
    }

    #[test]
    fn success_prints_nothing_and_exits_zero() {
        assert_eq!(outcome("status", Ok(())), (ExitCode::SUCCESS, None));
    }

    #[test]
    fn an_unknown_or_missing_command_runs_nothing_and_does_not_exit_zero() {
        for args in [
            &[][..],
            &["aply"],
            &[""],
            &["resolve", "0007_index_orders", "--skip", "--reason", "why"],
        ] {
            let mut ran = false;
            let exit = run_main(args, |_| {
                ran = true;
                Ok(())
            });
            assert_eq!(exit, ExitCode::from(2), "{args:?}");
            assert!(!ran, "{args:?} ran a command");
        }
    }

    #[test]
    fn verify_is_a_command() {
        let mut seen = None;
        let exit = run_main(&["verify", "main"], |command| {
            seen = Some(command);
            Ok(())
        });
        assert_eq!(exit, ExitCode::SUCCESS);
        assert_eq!(
            seen,
            Some(MigrateCommand::Verify {
                base: "main".to_owned()
            })
        );
    }

    #[test]
    fn the_named_command_is_the_one_run() {
        let mut seen = None;
        let exit = run_main(&["check"], |command| {
            seen = Some(command);
            Ok(())
        });
        assert_eq!(exit, ExitCode::SUCCESS);
        assert_eq!(seen, Some(MigrateCommand::Check));
    }
}
