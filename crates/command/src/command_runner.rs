//! [`CommandRunner`], which holds a set of [`Command`]s and dispatches one by address.

use std::collections::HashMap;

use tracing::{Instrument, error, info, info_span, instrument};

use crate::{Command, CommandArgs, CommandError, CommandListing};

/// Collects [`Command`]s and runs exactly one, by address, against a context.
#[derive(Debug)]
#[must_use]
pub struct CommandRunner<C> {
    commands: Vec<Command<C>>,
}

impl<C> CommandRunner<C> {
    /// Creates an empty runner.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    /// Adds a single command; duplicates are refused at dispatch and listing time.
    pub fn register(mut self, command: Command<C>) -> Self {
        self.commands.push(command);
        self
    }

    /// Adds every command from an iterator, returning the runner for chaining.
    pub fn extend(mut self, commands: impl IntoIterator<Item = Command<C>>) -> Self {
        self.commands.extend(commands);
        self
    }

    /// The number of registered commands.
    #[must_use]
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether no commands are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// The registered command addresses, sorted.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError::DuplicateCommand`] if two commands share an address.
    pub fn addresses(&self) -> Result<Vec<String>, CommandError> {
        self.index()?;
        Ok(self.sorted_addresses())
    }

    /// The registered commands as [`CommandListing`]s, sorted by address.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError::DuplicateCommand`] if two commands share an address.
    pub fn listings(&self) -> Result<Vec<CommandListing>, CommandError> {
        self.index()?;
        let mut rows: Vec<CommandListing> = self
            .commands
            .iter()
            .map(|c| CommandListing {
                address: c.address(),
                namespace: c.namespace(),
                name: c.name(),
                description: c.description(),
            })
            .collect();
        rows.sort_unstable_by(|a, b| a.address.cmp(&b.address));
        Ok(rows)
    }

    /// Runs the command addressed `address` against `ctx` with the passthrough `args`.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError::DuplicateCommand`] if two commands share an address,
    /// [`CommandError::UnknownCommand`] if none matches `address`, or whatever error
    /// the command itself returns.
    #[instrument(skip_all, fields(command = address, arg_count = args.len()))]
    pub async fn run(&self, address: &str, ctx: &C, args: CommandArgs) -> Result<(), CommandError>
    where
        C: Sync,
    {
        let index = self.index()?;
        let Some(&idx) = index.get(address) else {
            let available = self.sorted_addresses().join(", ");
            error!(command = address, available = %available, "unknown command");
            return Err(CommandError::UnknownCommand {
                name: address.to_string(),
                available,
            });
        };

        let command = &self.commands[idx];
        let span = info_span!("command", name = %command.address());
        async {
            info!("running command");
            if let Err(error) = command.run(ctx, args).await {
                error!(error = ?error, "command failed");
                return Err(error);
            }
            info!("command complete");
            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Builds an address-to-index map, erroring on the first duplicate.
    fn index(&self) -> Result<HashMap<String, usize>, CommandError> {
        let mut map = HashMap::with_capacity(self.commands.len());
        for (i, command) in self.commands.iter().enumerate() {
            let address = command.address();
            if map.insert(address, i).is_some() {
                return Err(CommandError::DuplicateCommand {
                    name: command.address(),
                });
            }
        }
        Ok(map)
    }

    fn sorted_addresses(&self) -> Vec<String> {
        let mut addresses: Vec<String> = self.commands.iter().map(Command::address).collect();
        addresses.sort_unstable();
        addresses
    }
}

impl<C> Default for CommandRunner<C> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn ok(
        namespace: &'static str,
        name: &'static str,
        description: Option<&'static str>,
    ) -> Command<()> {
        Command::new(namespace, name, description, |_ctx, _args| {
            Box::pin(async { Ok(()) })
        })
    }

    #[test]
    fn empty_runner_reports_empty() {
        let runner: CommandRunner<()> = CommandRunner::new();
        assert!(runner.is_empty());
        assert_eq!(runner.len(), 0);
        assert_eq!(
            runner.addresses().expect("no duplicates"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn addresses_and_listings_are_sorted() {
        let runner = CommandRunner::new()
            .register(ok("zeta", "last", None))
            .register(ok("alpha", "first", Some("first")));
        assert_eq!(
            runner.addresses().expect("unique"),
            vec!["alpha:first".to_string(), "zeta:last".to_string()]
        );
        let listings = runner.listings().expect("unique");
        assert_eq!(listings[0].address, "alpha:first");
        assert_eq!(listings[0].namespace, "alpha");
        assert_eq!(listings[0].description, Some("first"));
        assert_eq!(listings[1].address, "zeta:last");
    }

    #[test]
    fn one_name_in_two_namespaces_is_not_a_duplicate() {
        let runner = CommandRunner::new()
            .register(ok("business", "session-keys", None))
            .register(ok("holder", "session-keys", None));
        assert_eq!(
            runner.addresses().expect("distinct addresses"),
            vec![
                "business:session-keys".to_string(),
                "holder:session-keys".to_string()
            ]
        );
    }

    #[test]
    fn duplicate_addresses_are_rejected_by_listing() {
        let runner = CommandRunner::new()
            .register(ok("mint", "dup", None))
            .register(ok("mint", "dup", None));
        let err = runner
            .addresses()
            .expect_err("a duplicate has no unique listing");
        assert!(matches!(err, CommandError::DuplicateCommand { name } if name == "mint:dup"));
    }

    #[tokio::test]
    async fn run_dispatches_the_addressed_command() {
        struct Ctx {
            ran: Mutex<Vec<&'static str>>,
        }
        #[expect(clippy::excessive_nesting, reason = "readability baseline")]
        fn record(name: &'static str) -> Command<Ctx> {
            match name {
                "first" => Command::new("test", "first", None, |ctx: &Ctx, _args| {
                    Box::pin(async move {
                        ctx.ran.lock().unwrap().push("first");
                        Ok(())
                    })
                }),
                _ => Command::new("test", "second", None, |ctx: &Ctx, _args| {
                    Box::pin(async move {
                        ctx.ran.lock().unwrap().push("second");
                        Ok(())
                    })
                }),
            }
        }
        let ctx = Ctx {
            ran: Mutex::new(Vec::new()),
        };
        let runner = CommandRunner::new()
            .register(record("first"))
            .register(record("second"));
        runner
            .run("test:second", &ctx, CommandArgs::default())
            .await
            .expect("dispatch succeeds");
        assert_eq!(*ctx.ran.lock().unwrap(), vec!["second"]);
    }

    #[tokio::test]
    async fn a_bare_name_does_not_dispatch() {
        let runner = CommandRunner::new().register(ok("test", "echo", None));
        let err = runner
            .run("echo", &(), CommandArgs::default())
            .await
            .expect_err("a name without its namespace is not an address");
        assert!(matches!(err, CommandError::UnknownCommand { .. }));
    }

    #[tokio::test]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn run_forwards_passthrough_args() {
        struct Ctx {
            seen: Mutex<Vec<String>>,
        }
        let runner = CommandRunner::new().register(Command::new(
            "test",
            "echo",
            None,
            |ctx: &Ctx, args: CommandArgs| {
                Box::pin(async move {
                    ctx.seen.lock().unwrap().extend(args.into_vec());
                    Ok(())
                })
            },
        ));
        let ctx = Ctx {
            seen: Mutex::new(Vec::new()),
        };
        runner
            .run(
                "test:echo",
                &ctx,
                CommandArgs::new(["--flag".to_string(), "value".to_string()]),
            )
            .await
            .expect("dispatch succeeds");
        assert_eq!(*ctx.seen.lock().unwrap(), vec!["--flag", "value"]);
    }

    #[tokio::test]
    async fn unknown_command_reports_the_available_set() {
        let runner = CommandRunner::new()
            .register(ok("index", "backfill", None))
            .register(ok("greet", "hello", None));
        let err = runner
            .run("index:reindex", &(), CommandArgs::default())
            .await
            .expect_err("no such command");
        match err {
            CommandError::UnknownCommand { name, available } => {
                assert_eq!(name, "index:reindex");
                assert_eq!(available, "greet:hello, index:backfill");
            }
            other => panic!("expected UnknownCommand, got {other:?}"),
        }
    }

    #[tokio::test]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn run_propagates_the_commands_error() {
        let runner =
            CommandRunner::new().register(Command::new("test", "boom", None, |_ctx, _args| {
                Box::pin(async { Err(CommandError::msg("kaboom")) })
            }));
        let err = runner
            .run("test:boom", &(), CommandArgs::default())
            .await
            .expect_err("the command fails");
        assert!(matches!(err, CommandError::Message { .. }));
    }

    #[tokio::test]
    async fn run_rejects_duplicate_registration() {
        let runner = CommandRunner::new()
            .register(ok("mint", "dup", None))
            .register(ok("mint", "dup", None));
        let err = runner
            .run("mint:dup", &(), CommandArgs::default())
            .await
            .expect_err("ambiguous dispatch");
        assert!(matches!(err, CommandError::DuplicateCommand { .. }));
    }
}
