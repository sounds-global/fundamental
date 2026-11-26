//! `fun local`: the verbs, and what each prints.

use std::env;
use std::net::ToSocketAddrs;

use fundamental::Error as _;
use fundamental_local::{
    Checkout, InstanceNames, LocalError, LocalMachine, LocalProject, MachineResolver, MachineSetup,
    RegistryStore,
};
use time::OffsetDateTime;
use tracing::instrument;

use super::LocalArgs;
use super::input::{InstanceArgs, LocalVerb};
use crate::emit::emit;
use crate::{CliError, CliResult, CommandContext};

/// Execute `fun local <verb>`.
///
/// # Errors
/// [`CliError::Local`] carrying the verb's `LOCAL_0xx` refusal.
#[instrument(skip_all, err(Debug))]
#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "readability baseline"
)]
pub(crate) fn run(args: LocalArgs, ctx: CommandContext) -> CliResult<()> {
    let ui = ctx.ui();
    match args.verb {
        LocalVerb::Setup(setup) if setup.print => {
            let (_, files) = MachineSetup::plan()?;
            for (path, contents) in files.files() {
                emit(&format!("# {}", path.display()));
                for line in contents.lines() {
                    emit(line);
                }
                emit("");
            }
        }
        LocalVerb::Setup(_) => {
            let machine = LocalMachine::for_user()?;
            let report = MachineSetup::run(&RegistryStore::new(machine.store().state().clone()))?;
            ui.section(format!(
                "Routed .test to this machine's resolver ({:?})",
                report.platform
            ));
            for path in &report.written {
                ui.detail(format!("wrote {}", path.display()));
            }
            ui.note("every project and apex under .test now resolves without sudo");
        }
        LocalVerb::Trust => {
            let machine = LocalMachine::for_user()?;
            let project = LocalProject::load(&env::current_dir().map_err(io)?)?;
            let plan = machine.plan_trust(&project, OffsetDateTime::now_utc())?;
            ui.section(format!("Trusting the root of `{}`", project.name()));
            ui.detail(format!("SHA-256 {}", plan.fingerprint().colons()));
            ui.detail(format!(
                "it can sign only names under {}",
                plan.permitted().join(", ")
            ));
            for store in plan.stores() {
                ui.detail(format!("it goes into {store}"));
            }
            let report = machine.trust(&plan)?;
            for store in &report.stores {
                ui.detail(format!("trusted in {store}"));
            }
            if plan.minted() {
                ui.note("a new root was minted for this; run `fun local up` in each checkout");
            }
        }
        LocalVerb::Up(instance) => {
            let (machine, project, checkout) = open(&instance)?;
            let report = machine.up(&project, &checkout, OffsetDateTime::now_utc())?;
            ui.section(format!(
                "`{}` ({}) serves on {}",
                project.name(),
                checkout.role(),
                report.facts.address
            ));
            for (_, suffix) in &report.facts.suffixes {
                ui.detail(format!("https://*.{suffix}"));
            }
            if report.minted {
                ui.detail("minted the project's root");
            }
            if report.issued {
                ui.detail(format!("issued and proved {}", report.facts.tls.display()));
            }
            for warning in &report.warnings {
                ui.status(ui.warn(&format!("{}: {warning}", warning.error_code())));
            }
        }
        LocalVerb::Env(args) if args.names => {
            let project = LocalProject::load(&env::current_dir().map_err(io)?)?;
            let checkout = Checkout::detect(project.root(), args.instance.label.as_deref())?;
            for line in InstanceNames::new(&project, &checkout).shell().lines() {
                emit(line);
            }
        }
        LocalVerb::Env(args) => {
            let (machine, project, checkout) = open(&args.instance)?;
            let facts = machine.facts(&project, &checkout)?;
            for line in facts.shell().lines() {
                emit(line);
            }
            for (var, url) in project.origins().variables(&facts) {
                emit(&format!("{var}='{url}'"));
            }
        }
        LocalVerb::Status(instance) => {
            let (machine, project, checkout) = open(&instance)?;
            let registry = machine.store().read()?;
            ui.section("This machine's local origins");
            let routed = MachineResolver::routed(machine.platform());
            ui.detail(format!(
                ".test routed to the resolver: {}",
                if routed {
                    ui.ok("yes")
                } else {
                    ui.warn("no — run `fun local setup`")
                }
            ));
            let rows: Vec<Vec<String>> = registry
                .instances()
                .map(|(key, entry)| {
                    vec![
                        key.project().to_string(),
                        key.role().to_string(),
                        entry.address().to_string(),
                        entry.checkout().display().to_string(),
                    ]
                })
                .collect();
            ui.table(&["PROJECT", "INSTANCE", "ADDRESS", "CHECKOUT"], &rows);
            match machine.facts(&project, &checkout) {
                Ok(facts) => {
                    ui.section(format!(
                        "This checkout: {} on {}",
                        checkout.role(),
                        facts.address
                    ));
                    for (_, suffix) in &facts.suffixes {
                        let name = suffix.name(Some("fun-local-probe"));
                        let resolves = (name.as_str(), 443)
                            .to_socket_addrs()
                            .is_ok_and(|mut found| found.any(|found| found.ip() == facts.ip()));
                        ui.detail(format!(
                            "*.{suffix} resolves through the system: {}",
                            if resolves {
                                ui.ok("yes")
                            } else {
                                ui.warn("no")
                            }
                        ));
                    }
                    for (store, held) in machine.trusted(&project, &facts)? {
                        ui.detail(format!(
                            "root trusted in {}: {}",
                            store.describe(),
                            if held {
                                ui.ok("yes")
                            } else {
                                ui.warn("no — run `fun local trust`")
                            }
                        ));
                    }
                }
                Err(LocalError::InstanceNotUp) => {
                    ui.note("this checkout holds no instance; `fun local up` claims one");
                }
                Err(error) => {
                    ui.status(ui.warn(&format!("{}: {error}", error.error_code())));
                }
            }
        }
        LocalVerb::Forget(instance) => {
            let (machine, project, checkout) = open(&instance)?;
            match machine.forget(&project, &checkout)? {
                Some(address) => ui.section(format!("Gave back {address} and its names")),
                None => ui.status("this checkout held no instance; nothing to give back"),
            }
        }
    }
    Ok(())
}

fn open(instance: &InstanceArgs) -> CliResult<(LocalMachine, LocalProject, Checkout)> {
    let machine = LocalMachine::for_user()?;
    let project = LocalProject::load(&env::current_dir().map_err(io)?)?;
    let checkout = Checkout::detect(project.root(), instance.label.as_deref())?;
    Ok((machine, project, checkout))
}

fn io(error: std::io::Error) -> CliError {
    CliError::Local(fundamental_local::LocalError::Io {
        path: ".".to_owned(),
        action: "read as the working directory",
        cause: error,
    })
}
