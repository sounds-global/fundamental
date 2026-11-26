//! The top-level [`clap`] parser for the `fun` tool, and its dispatch.

use clap::Parser;

use crate::{CliResult, Command, context::CommandContext};

#[derive(Debug, Parser)]
#[command(
    name = "Fundamental",
    author = "Angelo Fornerino <angelo@sounds.global>",
    version = crate::version::VERSION,
    about = "A Rust framework for building secure, multi-surface applications",
    propagate_version = true
)]
/// Top-level command-line interface for the `fun` tool.
pub struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Suppress all output except errors
    #[arg(short, long, global = true)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Build the shared [`CommandContext`] and dispatch to the selected subcommand's handler.
    pub fn run(self) -> CliResult<()> {
        let ctx = CommandContext::new(self.quiet);

        match self.command {
            Command::New(args) => crate::commands::new::run(args, ctx),
            Command::Add(args) => crate::commands::add::run(args, ctx),
            Command::Generate(args) => crate::commands::generate::run(args, ctx),
            Command::Migrate(args) => crate::commands::migrate::run(args, ctx),
            Command::Seed(args) => crate::commands::seed::run(args, ctx),
            Command::Run(args) => crate::commands::run::run(args, ctx),
            Command::Manifest(args) => crate::commands::manifest::run(args, ctx),
            Command::Test(args) => crate::commands::test::run(args, ctx),
            Command::Bench(args) => crate::commands::bench::run(args, ctx),
            Command::Local(args) => crate::commands::local::run(args, ctx),
            Command::Build(args) => crate::commands::build::run(args, &ctx),
            Command::Package(args) => crate::commands::package::run(args, &ctx),
            Command::Dev(args) => crate::commands::dev::run(args, ctx),
            Command::Authz(args) => crate::commands::authz::run(args, ctx),
            Command::Keys(args) => crate::commands::keys::run(&args, &ctx),
            Command::Messages(args) => crate::commands::messages::run(args, ctx),
            Command::External(words) => crate::commands::external::run(words),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::panic::catch_unwind;

    use clap::{Command as ClapCommand, CommandFactory};

    use super::*;

    fn verbs(command: &clap::Command, path: &[String], found: &mut Vec<Vec<String>>) {
        let here: Vec<String> = path
            .iter()
            .cloned()
            .chain(std::iter::once(command.get_name().to_owned()))
            .collect();
        let mut children = command.get_subcommands().peekable();
        if children.peek().is_none() {
            found.push(here);
            return;
        }
        for child in children {
            verbs(child, &here, found);
        }
    }

    /// `debug_assert` misses a verb whose own `--quiet` shadows the global one, so every verb is parsed.
    #[test]
    fn every_verb_parses_without_panicking() {
        Cli::command().debug_assert();
        let mut found = Vec::new();
        verbs(&Cli::command(), &[], &mut found);
        let panicked: Vec<String> = found
            .into_iter()
            .filter(|verb| {
                let argv = verb.clone();
                catch_unwind(move || {
                    let _ = Cli::try_parse_from(argv);
                })
                .is_err()
            })
            .map(|verb| verb.join(" "))
            .collect();
        assert!(
            panicked.is_empty(),
            "these verbs panic when parsed: {panicked:?}"
        );
    }

    fn flags_in(text: &str) -> Vec<String> {
        let bytes = text.as_bytes();
        let mut named = Vec::new();
        let mut at = 0;
        while let Some(offset) = text[at..].find("--") {
            let start = at + offset;
            let inside_a_word =
                start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'-');
            let name: String = text[start + 2..]
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
                .collect();
            if !inside_a_word && name.starts_with(|c: char| c.is_ascii_lowercase()) {
                named.push(name.trim_end_matches('-').to_owned());
            }
            at = start + 2;
        }
        named
    }

    fn accepted(command: &ClapCommand) -> BTreeSet<String> {
        command
            .get_arguments()
            .flat_map(|arg| {
                arg.get_long_and_visible_aliases()
                    .unwrap_or_default()
                    .into_iter()
                    .map(str::to_owned)
            })
            .chain(["help", "version"].map(str::to_owned))
            .collect()
    }

    /// The command a `fun …` command line names, and how many of its words named it.
    fn resolve<'c>(root: &'c ClapCommand, words: &[&str]) -> (&'c ClapCommand, usize) {
        let mut command = root;
        let mut used = 0;
        for word in words {
            match command.find_subcommand(word) {
                Some(sub) => {
                    command = sub;
                    used += 1;
                }
                None => break,
            }
        }
        (command, used)
    }

    /// A flag is this verb's own unless a code span begins `fun …` (that verb's) or another program.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn check(root: &ClapCommand, command: &ClapCommand, path: &str, stray: &mut Vec<String>) {
        let help = command.clone().render_long_help().to_string();
        let own = accepted(command);
        for (index, piece) in help.split('`').enumerate() {
            let is_span = index % 2 == 1;
            let line = piece.trim_start().trim_start_matches("-- ").trim_start();
            let words: Vec<&str> = line.split_whitespace().collect();
            let (named_path, accepting) = if !is_span || line.starts_with('-') {
                (path.to_owned(), own.clone())
            } else if words.first() == Some(&"fun") {
                let (target, used) = resolve(root, &words[1..]);
                (words[..=used].join(" "), accepted(target))
            } else {
                continue;
            };
            for flag in flags_in(piece) {
                if !accepting.contains(&flag) {
                    stray.push(format!(
                        "`{path} --help` names --{flag}, which `{named_path}` does not accept"
                    ));
                }
            }
        }
    }

    fn walk(root: &ClapCommand, command: &ClapCommand, path: &str, stray: &mut Vec<String>) {
        check(root, command, path, stray);
        for sub in command.get_subcommands() {
            walk(root, sub, &format!("{path} {}", sub.get_name()), stray);
        }
    }

    #[test]
    fn every_flag_a_help_text_names_is_one_its_command_parses() {
        let mut root = Cli::command();
        root.build();
        let mut stray = Vec::new();
        walk(&root, &root, "fun", &mut stray);
        assert!(stray.is_empty(), "{}", stray.join("\n"));
    }

    #[test]
    fn a_flag_no_parser_accepts_is_caught_in_prose_and_in_a_fun_command_line() {
        let root = ClapCommand::new("fun")
            .subcommand(ClapCommand::new("dev").arg(clap::Arg::new("watch").long("watch")))
            .subcommand(
                ClapCommand::new("build")
                    .about("Builds.")
                    .long_about("Like `fun dev --watch`, `cargo build --features x`; not `fun dev --no-watch`, not --bundle"),
            );
        let mut stray = Vec::new();
        walk(&root, &root, "fun", &mut stray);
        assert_eq!(
            stray,
            [
                "`fun build --help` names --no-watch, which `fun dev` does not accept",
                "`fun build --help` names --bundle, which `fun build` does not accept",
            ]
        );
    }
}
