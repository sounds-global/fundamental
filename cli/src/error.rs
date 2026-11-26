//! The [`CliError`] type returned by `fun` command handlers.

use fundamental::Error;
use fundamental::authz::AuthzError;
use fundamental::core::validate::ValidationErrors;
use fundamental::nats::NatsError;
use fundamental::seed::SeedError;

/// Errors returned by `fun` command handlers, each with a stable `CLI_0xx` code.
#[derive(Debug, Error)]
pub(crate) enum CliError {
    /// `fun <name>` names no command of `fun`'s and no `fun-<name>` program on `PATH`.
    #[error(
        code = "CLI_078",
        message = "`fun {name}` is no command of fun's, and no fun-{name} program is on PATH; `fun --help` lists fun's own",
        status = 404
    )]
    NoSuchCommand { name: String },

    /// The target directory for a new project already exists.
    #[error(
        code = "CLI_001",
        message = "Directory already exists",
        developer_message = "Directory already exists: {path}",
        status = 400
    )]
    DirectoryExists { path: String },

    /// The supplied project name failed validation.
    #[error(
        code = "CLI_002",
        message = "Invalid project name: {reason}",
        developer_message = "Invalid project name '{name}': {reason}",
        status = 400
    )]
    InvalidProjectName { name: String, reason: String },

    /// An interactive prompt could not be completed.
    #[error(code = "CLI_003", message = "Prompt failed: {reason}", status = 400)]
    PromptFailed { reason: String },

    /// One or more validation checks failed.
    #[error(code = "CLI_004", message = "Validation failed: {0}", status = 400)]
    Validation(
        #[from]
        #[source]
        ValidationErrors,
    ),

    /// The environment gate refused seeding; forwards the `SEED_0xx` code.
    #[error(transparent)]
    Seed(#[from] SeedError),

    /// A web bundle build failed; forwards the `BUILD_0xx` code.
    #[error(transparent)]
    Build(#[from] fundamental::build::BuildError),

    /// A package could not be made. Transparent: forwards the `PKG_0xx` code.
    #[error(transparent)]
    Package(#[from] fundamental_package::PackageError),

    /// The project's manifest could not be read, or an edit to it was refused.
    #[error(transparent)]
    Manifest(#[from] fundamental_manifest::ManifestError),

    /// `fun manifest show` was asked for a key the manifest does not have.
    #[error(
        code = "CLI_056",
        message = "The manifest has no `{key}`",
        developer_message = "No file of the manifest defines `{key}`; `fun manifest show` prints a key that is written",
        status = 404
    )]
    ManifestKeyAbsent { key: String },

    /// `fun test` ran, and tests failed; each is named above the error.
    #[error(
        code = "CLI_059",
        message = "{failed} test(s) failed, each named above; the whole report is {report}",
        status = 422
    )]
    TestsFailed { failed: usize, report: String },

    /// `fun test` has no verdict: nextest could not run, or left no readable report.
    #[error(
        code = "CLI_060",
        message = "The tests have no verdict: {reason}",
        status = 500
    )]
    TestsDidNotRun { reason: String },

    /// `fun bench` could not run a layer: no bench target, a missing tool, or unreadable counts.
    #[error(
        code = "CLI_082",
        message = "The benchmarks did not run: {reason}",
        status = 500
    )]
    BenchDidNotRun { reason: String },

    /// A bench program of `fun bench` failed; its own output above says why.
    #[error(
        code = "CLI_083",
        message = "the {layer} bench of {package} failed; its output above says why",
        status = 422
    )]
    BenchFailed { layer: String, package: String },

    /// The instruction counts differ from the committed baseline.
    #[error(code = "CLI_084", message = "{reason}", status = 422)]
    BenchRegressed { reason: String },

    /// `fun package` was run on a checkout that differs from its commit.
    #[error(
        code = "CLI_072",
        message = "the checkout differs from its commit ({changes}); a package names the commit it was built from, so commit or set aside the changes first",
        status = 409
    )]
    PackageFromChangedTree { changes: String },

    /// `fun keys mint` was asked for a key version whose file already exists.
    #[error(
        code = "CLI_073",
        message = "{path} already holds a key; a key is never overwritten, because what it sealed would no longer open. Mint the next version instead",
        status = 409
    )]
    KeyExists { path: String },

    /// `fun keys mint` could not write its key file.
    #[error(
        code = "CLI_074",
        message = "the key file {path} could not be written: {reason}",
        status = 500
    )]
    KeyUnwritable { path: String, reason: String },

    /// `fun keys mint` was given a purpose that is not one plain name.
    #[error(
        code = "CLI_075",
        message = "`{purpose}` is not a key purpose: a purpose is one name of letters, digits, dots, dashes and underscores, as the code that reads it spells it",
        status = 400
    )]
    KeyPurposeInvalid { purpose: String },

    /// A crate's messages, or the workspace's list of crates, could not be read.
    #[error(
        code = "CLI_076",
        message = "Could not read the messages of {package}: {problems}",
        status = 500
    )]
    MessagesUnreadable { package: String, problems: String },

    /// The project's policy makes some of its messages' gaps errors.
    #[error(
        code = "CLI_077",
        message = "{count} message gap(s) are errors under the project's policy",
        status = 400
    )]
    MessagesRefused { count: usize },

    /// Local origins could not be set up, trusted, claimed or given back; forwards `LOCAL_0xx`.
    #[error(transparent)]
    Local(#[from] fundamental_local::LocalError),

    /// The seeder binary could not be launched.
    #[error(
        code = "CLI_005",
        message = "Could not run the seeder binary: {reason}",
        developer_message = "Failed to spawn `cargo run --bin {bin}`: {reason}",
        status = 500
    )]
    SeedSpawn { bin: String, reason: String },

    /// The seeder binary ran but exited with a non-zero status.
    #[error(
        code = "CLI_006",
        message = "Seeding failed (the seeder exited with code {code})",
        status = 500
    )]
    SeedFailed { code: i32 },

    /// The migration binary could not be launched.
    #[error(
        code = "CLI_017",
        message = "Could not run the migration binary: {reason}",
        developer_message = "Failed to spawn `cargo run --bin {bin}`: {reason}",
        status = 500
    )]
    MigrateSpawn { bin: String, reason: String },

    /// `fun migrate verify` could not name the base its branch is verified against.
    #[error(
        code = "CLI_079",
        message = "The base {base} names no commit to verify against: {reason}; pass one with --base <revision>",
        status = 400
    )]
    VerifyBaseUnresolved { base: String, reason: String },

    /// `fun migrate verify` found no app with a migration binary to verify.
    #[error(
        code = "CLI_080",
        message = "No app declares a migration binary (`<app>-migrate` or `migrate`), so there is nothing to verify",
        status = 404
    )]
    NoMigrator,

    /// `fun migrate rebase` could not name the commit its branch is rebased on.
    #[error(
        code = "CLI_081",
        message = "The upstream {upstream} names no commit to rebase on: {reason}; fetch it (`git fetch origin`), or name one: `fun migrate rebase <upstream>`",
        status = 400
    )]
    RebaseUpstreamUnresolved { upstream: String, reason: String },

    /// The migration binary ran but exited with a non-zero status.
    #[error(
        code = "CLI_018",
        message = "Migration failed (the migrate binary exited with code {code})",
        status = 500
    )]
    MigrateFailed { code: i32 },

    /// A file or directory could not be written while scaffolding a project.
    #[error(
        code = "CLI_007",
        message = "Could not write project files",
        developer_message = "Failed to write {path}: {reason}",
        status = 500
    )]
    Io { path: String, reason: String },

    /// The command was run outside any Fundamental project.
    #[error(
        code = "CLI_008",
        message = "Not inside a Fundamental project (no fun.toml found in this or any parent directory)",
        developer_message = "Searched from {start} upward and found no fun.toml",
        status = 400
    )]
    NotInProject { start: String },

    /// The named app does not exist in `fun.toml`.
    #[error(
        code = "CLI_009",
        message = "App '{app}' not found in fun.toml",
        status = 400
    )]
    AppNotFound { app: String },

    /// The item being added already exists in `fun.toml`.
    #[error(
        code = "CLI_010",
        message = "{what} already exists: {name}",
        developer_message = "{what} '{name}' is already defined in fun.toml",
        status = 400
    )]
    AlreadyExists { what: String, name: String },

    /// The project's `fun.toml` could not be parsed.
    #[error(
        code = "CLI_011",
        message = "Could not parse fun.toml",
        developer_message = "Failed to parse {path}: {reason}",
        status = 500
    )]
    TomlParse { path: String, reason: String },

    /// A supplied model name was not a valid Rust type identifier.
    #[error(
        code = "CLI_012",
        message = "Invalid model name: {reason}",
        developer_message = "Invalid model name '{name}': {reason}",
        status = 400
    )]
    InvalidModelName { name: String, reason: String },

    /// A `-f name:type` field specification could not be parsed or validated.
    #[error(
        code = "CLI_013",
        message = "Invalid field '{spec}': {reason}",
        status = 400
    )]
    InvalidField { spec: String, reason: String },

    /// `fun seed` found several apps with seeds and no `--app`.
    #[error(
        code = "CLI_014",
        message = "Multiple apps define seeds ({apps}); pass --app <name> to choose one",
        status = 400
    )]
    AmbiguousSeedApp { apps: String },

    /// `fun new` was run non-interactively (`--yes`) without a project name.
    #[error(
        code = "CLI_015",
        message = "A project name is required",
        developer_message = "`fun new` needs a name argument when run with --yes",
        status = 400
    )]
    MissingProjectName,

    /// An unknown `--db` value was supplied to `fun new`.
    #[error(
        code = "CLI_016",
        message = "Unknown database '{value}'; expected none, sql, or cql",
        status = 400
    )]
    InvalidDatabase { value: String },

    /// The `fun dev` server process exited with a non-zero status.
    #[error(
        code = "CLI_022",
        message = "the dev server exited with status {code}",
        status = 500
    )]
    DevServerFailed { code: i32 },

    /// A web app's configured `css_command` exited with a non-zero status.
    #[error(
        code = "CLI_023",
        message = "the css_command exited with status {code}",
        status = 500
    )]
    CssCommandFailed { code: i32 },

    /// `fun dev`/`fun build` was run in a project that declares no apps.
    #[error(
        code = "CLI_024",
        message = "this project has no apps; add one with `fun add app <name>` or scaffold a surface with `fun new`",
        status = 400
    )]
    NoApps,

    /// `fun dev`/`fun build` found several apps and none was named.
    #[error(
        code = "CLI_025",
        message = "this project has multiple apps ({apps}); choose one, e.g. `fun dev <app>`",
        status = 400
    )]
    AmbiguousApps { apps: String },

    /// `fun generate` found no domain crate to scaffold into.
    #[error(
        code = "CLI_052",
        message = "no domain crate under {domain} — `fun generate` writes into a domain crate, and this project has none",
        status = 400
    )]
    NoDomainCrate { domain: String },

    /// `fun generate` found several domain crates and was not told which to use.
    #[error(
        code = "CLI_053",
        message = "several domain crates ({crates}) — name one with `--crate <name>` (`fun generate`) or `--domain <name>` (`fun add app`)",
        status = 400
    )]
    AmbiguousDomainCrate { crates: String },

    /// A generated app asks its domain crate for a feature the crate does not declare.
    #[error(
        code = "CLI_054",
        message = "the domain crate `{package}` declares no `{feature}` feature, which the app needs — add it to its [features]",
        status = 400
    )]
    DomainFeatureMissing { package: String, feature: String },

    /// `fun build`'s `cargo build` for the app exited with a non-zero status.
    #[error(
        code = "CLI_026",
        message = "the build exited with status {code}",
        status = 500
    )]
    BuildFailed { code: i32 },

    /// `fun dev <app> <surface>` named a surface the app does not declare.
    #[error(
        code = "CLI_027",
        message = "app '{app}' does not declare a '{surface}' surface",
        status = 400
    )]
    SurfaceNotDeclared { app: String, surface: String },

    /// `fun generate slice` found several databases and no `--db`.
    #[error(
        code = "CLI_028",
        message = "this project wires multiple databases ({engines}); choose the slice model's backend with `--db sql` or `--db cql`",
        status = 400
    )]
    AmbiguousDatabase { engines: String },

    /// `fun dev` mixed a runs-alone surface (native) with others in one run.
    #[error(
        code = "CLI_029",
        message = "the {surface} surface runs as its own app process; run `fun dev {app} {surface}` by itself (server surfaces run with `fun dev {app} <surface>...`)",
        status = 400
    )]
    SurfaceRunsAlone { app: String, surface: String },

    /// A mobile native flow needs the Tauri CLI and `cargo tauri` is not installed.
    #[error(
        code = "CLI_030",
        message = "the Tauri CLI is required for iOS/Android runs; install it with `cargo install tauri-cli --locked`",
        status = 500
    )]
    TauriCliMissing,

    /// The CLI reached a state its own control flow rules out: a bug in `fun`.
    #[error(
        code = "CLI_031",
        message = "internal error: {detail}; this is a bug in fun — please report it",
        status = 500
    )]
    Internal { detail: String },

    /// A `fun run` command could not be launched (the shell, or `cargo run`).
    #[error(
        code = "CLI_032",
        message = "Could not run command: {reason}",
        developer_message = "Failed to spawn `{bin}`: {reason}",
        status = 500
    )]
    CommandSpawn { bin: String, reason: String },

    /// A `fun run` command ran but exited with a non-zero status.
    #[error(
        code = "CLI_033",
        message = "Command failed (exited with code {code})",
        status = 500
    )]
    CommandFailed { code: i32 },

    // CLI_034 is retired, never reused.
    /// `fun run <address>` named a command in neither `[commands]` nor any app's `commands` binary.
    #[error(
        code = "CLI_035",
        message = "Unknown command '{name}'.{suggestion} Run `fun run --list` for all {count} addresses.",
        developer_message = "No `[commands]` entry and no `#[command]` addressed `{name}`",
        status = 404
    )]
    UnknownCommand {
        name: String,
        /// ``  Did you mean `<address>`? ``, or empty when nothing was close.
        suggestion: String,
        count: usize,
    },

    /// A schema definition file for `fun authz schema-validate` could not be read.
    #[error(
        code = "CLI_037",
        message = "Cannot read schema file '{path}': {reason}",
        status = 400
    )]
    AuthzSchemaRead { path: String, reason: String },

    /// An authorization operation failed; forwards the `AUTHZ_0xx` code.
    #[error(transparent)]
    Authz(#[from] AuthzError),

    /// A `fun authz` `namespace:id` argument was not in the expected shape.
    #[error(
        code = "CLI_038",
        message = "Invalid {kind} '{value}': expected `namespace:id`{userset_hint}",
        status = 400
    )]
    InvalidReference {
        kind: &'static str,
        value: String,
        /// A `subject`-only hint about the `namespace:id#relation` userset form.
        userset_hint: &'static str,
    },

    /// The async runtime a `fun authz` RPC needs could not be created.
    #[error(
        code = "CLI_039",
        message = "Cannot start the async runtime: {reason}",
        status = 500
    )]
    AuthzRuntime { reason: String },

    /// The authorization service answered with an error rather than a decision.
    #[error(
        code = "CLI_040",
        message = "Authorization service error [{code}]: {message}",
        status = 502
    )]
    AuthzServiceError { code: String, message: String },

    /// An app declares no binary for a role, as `<app>-<role>` or `<role>`.
    #[error(
        code = "CLI_041",
        message = "App `{app}` declares no `{role}` binary: expected a `[[bin]] name = \"{prefixed}\"` (or `\"{role}\"`) in {manifest}",
        status = 400
    )]
    AppBinMissing {
        app: String,
        role: String,
        /// The preferred, collision-free binary name.
        prefixed: String,
        manifest: String,
    },

    /// Two sources declared one `<namespace>:<name>`; refused rather than one shadowing the other.
    #[error(
        code = "CLI_049",
        message = "Two commands are addressed `{address}`: {first} and {second}. An address has one owner — rename one.",
        status = 400
    )]
    DuplicateCommandAddress {
        address: String,
        first: String,
        second: String,
    },

    /// A `[commands]` entry could not be read as a command.
    #[error(
        code = "CLI_050",
        message = "`[commands.{namespace}] {name}` in {path} {reason}",
        developer_message = "Each entry is `name = \"shell line\"` or `name = {{ run = \"…\", description = \"…\" }}`",
        status = 400
    )]
    ManifestCommandShape {
        path: String,
        namespace: String,
        name: String,
        reason: String,
    },

    /// A machine-readable answer could not be rendered.
    #[error(
        code = "CLI_051",
        message = "Could not serialise {what}: {reason}",
        status = 500
    )]
    Serialize { what: String, reason: String },

    /// A `fun authz` RPC could not reach the service; forwards the `NATS_0xx` code.
    #[error(transparent)]
    Nats(#[from] NatsError),

    /// A surface subset was named alongside more than one app.
    #[error(
        code = "CLI_042",
        message = "A surface subset applies to one app, but {apps} were named with [{surfaces}]: run one app with its surfaces, or the set without any",
        status = 400
    )]
    SurfaceSubsetNeedsOneApp { apps: String, surfaces: String },

    /// The development loop failed; forwards the `DEV_0xx` code.
    #[error(transparent)]
    Dev(#[from] fundamental_dev::DevError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental::config::{EnvironmentOrigin, resolve_environment};
    use fundamental::core::{BuildProfile, Environment};
    use fundamental::seed::guard_environment;

    #[test]
    fn a_refused_environment_forwards_its_seed_code_transparently() {
        let production = resolve_environment(
            EnvironmentOrigin::Chosen {
                environment: Environment::Production,
                name: None,
            },
            None,
            BuildProfile::Debug,
        )
        .expect("a chosen environment resolves");
        let seed_err = guard_environment(&production, false).expect_err("production is refused");
        let cli_err: CliError = seed_err.into();
        assert!(matches!(cli_err, CliError::Seed(_)));
        assert_eq!(cli_err.error_code(), "SEED_003");
        assert_eq!(cli_err.http_status_code(), 403);
    }

    #[test]
    fn seed_failure_carries_the_exit_code() {
        let err = CliError::SeedFailed { code: 42 };
        assert_eq!(err.error_code(), "CLI_006");
        assert!(err.to_string().contains("42"));
    }

    #[test]
    fn runs_alone_names_the_surface_and_the_solo_command() {
        let err = CliError::SurfaceRunsAlone {
            app: "desk".to_owned(),
            surface: "native".to_owned(),
        };
        assert_eq!(err.error_code(), "CLI_029");
        assert_eq!(err.http_status_code(), 400);
        let message = err.to_string();
        assert!(message.contains("fun dev desk native"), "{message}");
        assert!(message.contains("its own app process"), "{message}");
    }

    #[test]
    fn internal_errors_carry_a_stable_code_and_name_the_invariant() {
        let err = CliError::Internal {
            detail: "surface arm reached after its guard".to_owned(),
        };
        assert_eq!(err.error_code(), "CLI_031");
        assert_eq!(err.http_status_code(), 500);
        assert!(err.to_string().contains("surface arm reached"));
        assert!(err.to_string().contains("bug in fun"));
    }

    #[test]
    fn command_failure_carries_the_exit_code() {
        let err = CliError::CommandFailed { code: 3 };
        assert_eq!(err.error_code(), "CLI_033");
        assert_eq!(err.http_status_code(), 500);
        assert!(err.to_string().contains('3'));
    }

    #[test]
    fn unknown_command_carries_the_near_match() {
        let err = CliError::UnknownCommand {
            name: "bootstrap-keys".to_owned(),
            suggestion: " Did you mean `bootstrap:keys`?".to_owned(),
            count: 90,
        };
        assert_eq!(err.error_code(), "CLI_035");
        assert_eq!(err.http_status_code(), 404);
        assert!(err.to_string().contains("bootstrap:keys"));
        assert!(err.to_string().contains("90"));
    }

    #[test]
    fn unknown_command_says_nothing_extra_when_nothing_is_close() {
        let err = CliError::UnknownCommand {
            name: "aircraft-carrier".to_owned(),
            suggestion: String::new(),
            count: 90,
        };
        assert!(!err.to_string().contains("Did you mean"));
    }

    #[test]
    fn a_duplicate_address_names_both_sides() {
        let err = CliError::DuplicateCommandAddress {
            address: "bootstrap:seed".to_owned(),
            first: "fun.toml".to_owned(),
            second: "apps/layer0/src/bin/commands/bootstrap_seed.rs".to_owned(),
        };
        assert_eq!(err.error_code(), "CLI_049");
        assert_eq!(err.http_status_code(), 400);
        assert!(err.to_string().contains("fun.toml"));
        assert!(err.to_string().contains("bootstrap_seed.rs"));
    }

    #[test]
    fn a_malformed_manifest_command_names_its_address_and_file() {
        let err = CliError::ManifestCommandShape {
            path: "fun.toml".to_owned(),
            namespace: "gate".to_owned(),
            name: "docs".to_owned(),
            reason: "has no `run` string".to_owned(),
        };
        assert_eq!(err.error_code(), "CLI_050");
        assert_eq!(err.http_status_code(), 400);
        assert!(err.to_string().contains("gate"));
        assert!(err.to_string().contains("docs"));
    }
}
