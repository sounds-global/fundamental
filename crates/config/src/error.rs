//! [`ConfigError`], returned by every fallible configuration operation.

use std::path::PathBuf;

use fundamental_macros::Error;
use fundamental_manifest::ManifestError;

/// Configuration error type.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// Failed to read configuration file.
    #[error(
        code = "CFG_001",
        message = "failed to read config file '{}': {source}", path.display(),
        status = 500
    )]
    ReadFile {
        /// The path that couldn't be read.
        path: PathBuf,
        /// The underlying IO error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to parse TOML configuration.
    #[error(
        code = "CFG_002",
        message = "failed to parse config file '{}': {message}", path.display(),
        status = 500
    )]
    ParseToml {
        /// The path that couldn't be parsed.
        path: PathBuf,
        /// The parse error message.
        message: String,
    },

    /// Missing required configuration field.
    #[error(
        code = "CFG_003",
        message = "missing required configuration: {field}",
        status = 500
    )]
    MissingField {
        /// The missing field name.
        field: String,
    },

    /// Invalid configuration value.
    #[error(
        code = "CFG_004",
        message = "invalid configuration value for '{field}': {message}",
        status = 500
    )]
    InvalidValue {
        /// The field with invalid value.
        field: String,
        /// Description of why the value is invalid.
        message: String,
    },

    /// Environment variable not found.
    #[error(
        code = "CFG_005",
        message = "environment variable not found: {var}",
        status = 500
    )]
    EnvVarNotFound {
        /// The missing environment variable name.
        var: String,
    },

    /// Failed to expand environment variable in config value.
    #[error(
        code = "CFG_006",
        message = "failed to expand environment variable '{var}' in field '{field}'",
        status = 500
    )]
    EnvVarExpansion {
        /// The field containing the variable reference.
        field: String,
        /// The missing environment variable.
        var: String,
    },

    /// TLS configuration is required but not provided.
    #[error(
        code = "CFG_007",
        message = "TLS is required in {environment} environment but not configured",
        status = 500
    )]
    TlsRequired {
        /// The environment that requires TLS.
        environment: String,
    },

    /// Secret manager is required but not configured.
    #[error(
        code = "CFG_008",
        message = "secret manager is required in {environment} environment",
        status = 500
    )]
    SecretManagerRequired {
        /// The environment that requires a secret manager.
        environment: String,
    },

    /// Validation error.
    #[error(
        code = "CFG_009",
        message = "configuration validation failed: {message}",
        status = 500
    )]
    Validation {
        /// Description of the validation failure.
        message: String,
    },

    /// Multiple validation errors, every reason in the message.
    #[error(
        code = "CFG_010",
        message = "configuration validation failed with {count} errors: {errors:?}",
        status = 500
    )]
    MultipleValidationErrors {
        /// Number of errors.
        count: usize,
        /// Individual error messages.
        errors: Vec<String>,
    },

    /// Secret loading error.
    #[error(
        code = "CFG_011",
        message = "failed to load secret '{id}': {message}",
        status = 500
    )]
    SecretLoad {
        /// The secret identifier.
        id: String,
        /// Error message.
        message: String,
    },

    /// Configuration source error.
    #[error(
        code = "CFG_012",
        message = "configuration source error: {message}",
        status = 500
    )]
    Source {
        /// Error message.
        message: String,
    },

    /// Deserialization error for custom section.
    #[error(
        code = "CFG_013",
        message = "failed to deserialize custom section '{section}': {message}",
        status = 500
    )]
    CustomSection {
        /// The section name.
        section: String,
        /// Error message.
        message: String,
    },

    /// A `${file:…}` reference could not be read; the message names only the path.
    #[error(
        code = "CFG_014",
        message = "could not read the file referenced by '${{file:{path}}}': {message}",
        status = 500
    )]
    SecretFileUnreadable {
        /// The referenced path.
        path: String,
        /// Why it could not be read.
        message: String,
    },

    /// The deployment named an app instance the manifest does not declare.
    #[error(
        code = "CFG_015",
        message = "app instance '{instance}' is not declared: add [apps.{instance}] with instance_of = \"{binary}\"",
        status = 500
    )]
    AppInstanceUndeclared {
        /// The instance the deployment named.
        instance: String,
        /// The app the binary is.
        binary: String,
    },

    /// An instance section names a different base app than the binary booting it, or the
    /// binary's own section claims to be an instance.
    #[error(
        code = "CFG_016",
        message = "app '{instance}' declares instance_of = {declared}, but it is being booted by the '{binary}' binary",
        status = 500
    )]
    AppInstanceOfAnotherApp {
        /// The section being loaded.
        instance: String,
        /// What its `instance_of` says, or `nothing`.
        declared: String,
        /// The app the binary is.
        binary: String,
    },

    /// An instance restates a key its base app does not have.
    #[error(
        code = "CFG_017",
        message = "app instance '{instance}' sets '{key}', which its base app '{binary}' does not have",
        status = 500
    )]
    AppInstanceRestatesUnknownKey {
        /// The instance.
        instance: String,
        /// The dotted key path under the instance's section.
        key: String,
        /// The base app.
        binary: String,
    },

    /// An instance would inherit a secret-bearing value from its base — the base's key,
    /// credential or trusted issuer — instead of naming its own.
    #[error(
        code = "CFG_018",
        message = "app instance '{instance}' inherits '{key}' from '{binary}'; an instance must name its own secret there",
        status = 500
    )]
    AppInstanceInheritsSecret {
        /// The instance.
        instance: String,
        /// The dotted key path under the instance's section.
        key: String,
        /// The base app.
        binary: String,
    },

    /// A release build was started with `FUNDAMENTAL_ENV` unset or blank.
    #[error(
        code = "CFG_019",
        message = "FUNDAMENTAL_ENV is unset, and a release build does not assume an environment: set it to development, test, staging, production, or a custom name whose [environments.<name>] declares posture = \"deployed\" or \"development\"",
        status = 500
    )]
    EnvironmentUnset,

    /// A release build's custom environment name declares no posture in its manifest table.
    #[error(
        code = "CFG_020",
        message = "environment '{name}' is not a built-in and declares no posture: add posture = \"deployed\" or \"development\" to [environments.{name}] in fun.toml, or set FUNDAMENTAL_ENV to development, test, staging or production",
        status = 500
    )]
    EnvironmentPostureUndeclared {
        /// The custom environment name, as `FUNDAMENTAL_ENV` spelled it.
        name: String,
    },

    /// The manifest declares a posture it may not.
    #[error(
        code = "CFG_021",
        message = "[environments.{table}] cannot be used: {reason}",
        status = 500
    )]
    EnvironmentPostureConflict {
        /// The `[environments.*]` table at fault.
        table: String,
        /// What makes it unusable, and what to write instead.
        reason: String,
    },

    /// A manifest was named by a file other than `fun.toml`.
    #[error(
        code = "CFG_022",
        message = "'{}' is not a manifest: configuration reads a project's fun.toml", path.display(),
        status = 500
    )]
    NotAManifest {
        /// The path that was given.
        path: PathBuf,
    },

    /// An instance leaves out or redeclares a leaf naming its base's identity, so it would connect as its base.
    #[error(
        code = "CFG_023",
        message = "app instance '{instance}' inherits '{key}' from '{binary}'; an instance restates every leaf that names who it is",
        status = 500
    )]
    AppInstanceInheritsIdentity {
        /// The instance.
        instance: String,
        /// The dotted key path under the instance's section.
        key: String,
        /// The base app.
        binary: String,
    },

    /// An instance restates a value naming its base's identity verbatim, so it would connect as its base.
    #[error(
        code = "CFG_024",
        message = "app instance '{instance}' restates '{key}' with the value '{binary}' has; an instance names its own identity",
        status = 500
    )]
    AppInstanceSharesIdentity {
        /// The instance.
        instance: String,
        /// The dotted key path under the instance's section.
        key: String,
        /// The base app.
        binary: String,
    },

    /// An address was asked of a surface that listens on a passed socket, which has none.
    #[error(
        code = "CFG_025",
        message = "surface '{surface}' listens on a passed socket and has no address to bind",
        status = 500
    )]
    SurfaceListensActivated {
        /// The dotted surface key, `apps.<app>.<surface>`.
        surface: String,
    },

    /// A surface listens on a passed socket, and none of the passed sockets is named for it.
    #[error(
        code = "CFG_026",
        message = "surface '{surface}' listens on a passed socket, and none was passed under its name",
        status = 500
    )]
    ActivatedSocketMissing {
        /// The surface, which is the name its socket is passed under.
        surface: String,
    },

    /// The service manager passed a socket no surface of this app listens on.
    #[error(
        code = "CFG_027",
        message = "a socket was passed as '{name}', and no surface of this app listens on it",
        status = 500
    )]
    ActivatedSocketUnclaimed {
        /// The name the socket was passed under.
        name: String,
    },

    /// The passed sockets cannot be read by name.
    #[error(
        code = "CFG_028",
        message = "the passed sockets cannot be read: {reason}",
        status = 500
    )]
    ActivationMalformed {
        /// What is wrong with them.
        reason: String,
    },

    /// The manifest was read and refused.
    #[error(transparent)]
    Manifest(#[from] ManifestError),

    /// A file could not be replaced whole; the file is as it was.
    #[error(
        code = "CFG_029",
        message = "could not replace '{path}': {message}",
        status = 500
    )]
    FileNotReplaced {
        /// The file that was to be replaced.
        path: String,
        /// Why it was not.
        message: String,
    },

    /// A release's configuration was checked outside a deployed environment.
    #[error(
        code = "CFG_030",
        message = "a release is checked in the environment its units give it, and this one is {environment}: set FUNDAMENTAL_ENV as they set it",
        status = 500
    )]
    ReleaseNotDeployed {
        /// The environment the check was given.
        environment: String,
    },

    /// A deployed app's web surface names no bundle directory.
    #[error(
        code = "CFG_031",
        message = "the app '{app}' runs in {environment} and its web surface names no bundle: set `dir` under [apps.{app}.web] to the release's bundle",
        status = 500
    )]
    BundleUnnamed {
        /// The app whose web surface names none.
        app: String,
        /// The deployed environment it runs in.
        environment: String,
    },

    /// A deployed app's web bundle is not a directory holding anything.
    #[error(
        code = "CFG_032",
        message = "the app '{app}' serves its bundle from {dir}, which holds no bundle: check `dir` under [apps.{app}.web] against the release's package",
        status = 500
    )]
    BundleMissing {
        /// The app whose bundle is missing.
        app: String,
        /// Where it was looked for.
        dir: String,
    },

    /// `fun.toml` overrides settings for an environment, which its own file does.
    #[error(
        code = "CFG_033",
        message = "fun.toml overrides settings under [environments.{name}]: an environment's settings live in fun.{name}.toml beside it, and [environments.{name}] holds only its posture",
        status = 500
    )]
    EnvironmentOverridesInManifest {
        /// The environment whose table holds settings.
        name: String,
    },
}

impl ConfigError {
    /// Creates a missing field error.
    pub fn missing_field(field: impl Into<String>) -> Self {
        Self::MissingField {
            field: field.into(),
        }
    }

    /// Creates an invalid value error.
    pub fn invalid_value(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::InvalidValue {
            field: field.into(),
            message: message.into(),
        }
    }

    /// Creates an env var not found error.
    pub fn env_var_not_found(var: impl Into<String>) -> Self {
        Self::EnvVarNotFound { var: var.into() }
    }

    /// Creates a TLS required error.
    pub fn tls_required(environment: impl Into<String>) -> Self {
        Self::TlsRequired {
            environment: environment.into(),
        }
    }

    /// Creates a secret-manager-required error.
    pub fn secret_manager_required(environment: impl Into<String>) -> Self {
        Self::SecretManagerRequired {
            environment: environment.into(),
        }
    }

    /// Creates a validation error.
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation {
            message: message.into(),
        }
    }

    /// Creates a source error.
    pub fn source(message: impl Into<String>) -> Self {
        Self::Source {
            message: message.into(),
        }
    }

    /// Creates a custom section deserialization error.
    pub fn custom_section(section: impl Into<String>, message: impl Into<String>) -> Self {
        Self::CustomSection {
            section: section.into(),
            message: message.into(),
        }
    }
}
