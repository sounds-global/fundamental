//! [`MigrateCommand`] — what a migrate binary was asked to do.

use fundamental_db::migration::Resolve;

/// The subcommand a migrate binary runs, with its arguments: `apply`, `status`, `check`,
/// `verify <base>` or `resolve <name> (--retry | --applied) --reason <text>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrateCommand {
    /// Apply every pending migration.
    Apply,
    /// Report where each migration stands: applied, failed at a step, pending.
    Status,
    /// Refuse if a migration on disk is unapplied, and report schema drift.
    Check,
    /// Replay every migration into empty schemas, from scratch and as the upgrade from the base
    /// revision, and refuse any difference from the committed snapshots and the models
    /// ([`MigrateRunner::verify_sql`](crate::MigrateRunner::verify_sql)).
    Verify {
        /// The base revision; empty when none was given, which the runner refuses by name.
        base: String,
    },
    /// Settle the failed migration `name` by `act`, recording `reason`
    /// ([`MigrateRunner::resolve_sql`](crate::MigrateRunner::resolve_sql)).
    Resolve {
        /// The migration's ledger name.
        name: String,
        /// Run it again, or record it applied once the catalog shows it.
        act: Resolve,
        /// Why, as the resolution record keeps it; never empty.
        reason: String,
    },
}

impl MigrateCommand {
    /// The command `args` — the words after the binary's name — ask for, or `None` for
    /// anything else.
    #[must_use]
    pub fn parse<S: AsRef<str>>(args: &[S]) -> Option<Self> {
        let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
        match args.as_slice() {
            ["apply"] => Some(Self::Apply),
            ["status"] => Some(Self::Status),
            ["check"] => Some(Self::Check),
            ["verify"] => Some(Self::Verify {
                base: String::new(),
            }),
            ["verify", base] => Some(Self::Verify {
                base: (*base).to_owned(),
            }),
            ["resolve", name, flags @ ..] => resolve(name, flags),
            _ => None,
        }
    }

    /// The word that names it, for a log line.
    #[must_use]
    pub fn word(&self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::Status => "status",
            Self::Check => "check",
            Self::Verify { .. } => "verify",
            Self::Resolve { .. } => "resolve",
        }
    }
}

/// `resolve <name>`'s flags: exactly one of `--retry` and `--applied`, and `--reason` with a
/// text that is not empty, in any order, and nothing else.
fn resolve(name: &str, flags: &[&str]) -> Option<MigrateCommand> {
    if name.is_empty() || name.starts_with('-') {
        return None;
    }
    let mut act = None;
    let mut reason = None;
    let mut rest = flags.iter();
    while let Some(flag) = rest.next() {
        match *flag {
            "--retry" | "--applied" if act.is_none() => {
                act = Resolve::parse(&flag[2..]);
            }
            "--reason" if reason.is_none() => {
                reason = rest
                    .next()
                    .map(|text| text.trim())
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned);
                reason.as_ref()?;
            }
            _ => return None,
        }
    }
    Some(MigrateCommand::Resolve {
        name: name.to_owned(),
        act: act?,
        reason: reason?,
    })
}

#[cfg(test)]
mod tests {
    use fundamental_db::migration::Resolve;

    use super::MigrateCommand;

    #[test]
    fn each_word_is_its_command_and_nothing_else_is() {
        assert_eq!(
            MigrateCommand::parse(&["apply"]),
            Some(MigrateCommand::Apply)
        );
        assert_eq!(
            MigrateCommand::parse(&["status"]),
            Some(MigrateCommand::Status)
        );
        assert_eq!(
            MigrateCommand::parse(&["check"]),
            Some(MigrateCommand::Check)
        );
        assert_eq!(
            MigrateCommand::parse(&["verify", "main"]),
            Some(MigrateCommand::Verify {
                base: "main".to_owned()
            })
        );
        for other in [
            &[][..],
            &[""],
            &["Apply"],
            &["migrate"],
            &["apply "],
            &["Verify"],
            &["apply", "now"],
            &["verify", "main", "extra"],
        ] {
            assert_eq!(MigrateCommand::parse(other), None, "{other:?}");
        }
    }

    #[test]
    fn resolve_takes_one_act_and_a_reason_in_either_order() {
        let retry = Some(MigrateCommand::Resolve {
            name: "0007_index_orders".to_owned(),
            act: Resolve::Retry,
            reason: "timeout at peak".to_owned(),
        });
        assert_eq!(
            MigrateCommand::parse(&[
                "resolve",
                "0007_index_orders",
                "--retry",
                "--reason",
                "timeout at peak"
            ]),
            retry
        );
        assert_eq!(
            MigrateCommand::parse(&[
                "resolve",
                "0007_index_orders",
                "--reason",
                " timeout at peak ",
                "--retry"
            ]),
            retry
        );
        assert!(matches!(
            MigrateCommand::parse(&["resolve", "a", "--applied", "--reason", "built by hand"]),
            Some(MigrateCommand::Resolve {
                act: Resolve::Applied,
                ..
            })
        ));
    }

    #[test]
    fn a_resolve_that_is_not_exactly_one_act_with_a_reason_is_refused() {
        for refused in [
            &["resolve", "a", "--retry", "--applied", "--reason", "why"][..],
            &["resolve", "a", "--reason", "why"],
            &["resolve", "a", "--retry"],
            &["resolve", "a", "--retry", "--reason"],
            &["resolve", "a", "--retry", "--reason", "  "],
            &[
                "resolve", "a", "--retry", "--reason", "why", "--reason", "again",
            ],
            &["resolve", "a", "--skip", "--reason", "why"],
            &["resolve", "a", "--rolled-back", "--reason", "why"],
            &["resolve", "a", "--retry", "--reason", "why", "--force"],
            &["resolve", "--retry", "--reason", "why"],
            &["resolve"],
        ] {
            assert_eq!(MigrateCommand::parse(refused), None, "{refused:?}");
        }
    }
}
