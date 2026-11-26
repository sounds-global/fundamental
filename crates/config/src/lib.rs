//! Typed, environment-aware configuration for Fundamental applications.
//!
//! ```rust,ignore
//! use fundamental_config::ConfigLoader;
//!
//! // Load configuration with auto-detected environment
//! let config = ConfigLoader::auto()
//!     .with_fundamental_toml("fun.toml")
//!     .with_env_file("config")
//!     .with_env_vars()
//!     .load()?;
//!
//! // Access configuration
//! println!("Environment: {}", config.environment);
//! println!("Server: {}:{}", config.server.host, config.server.port);
//!
//! // Validate configuration
//! config.validate()?;
//! ```
//!
//! ```rust,ignore
//! // Access individual values
//! let enabled = config.custom.get_bool("feature_flags.new_checkout")
//!     .unwrap_or(false);
//!
//! // Or deserialize into a typed struct
//! #[derive(Deserialize)]
//! struct PaymentConfig {
//!     provider: String,
//!     sandbox: bool,
//! }
//!
//! let payment: PaymentConfig = config.custom.section("payment")?;
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::all)]

pub mod loader;
pub mod settings;
pub mod source;

mod activation_check;
mod app_settings;
mod atomic_file;
mod deployable;
mod dev_vars_file;
mod dev_vars_for_app;
mod environment;
mod error;
mod result;
mod secret_file;

pub use app_settings::AppSettings;
pub use atomic_file::AtomicFile;
pub use deployable::Deployable;
pub use dev_vars_file::DevVarsFile;
pub use dev_vars_for_app::dev_vars_for_app;
pub use environment::{EnvironmentOrigin, ResolvedEnvironment, resolve_environment};
pub use error::ConfigError;
pub use loader::ConfigLoader;
pub use result::ConfigResult;
pub use secret_file::SecretFile;
pub use settings::FundamentalConfig;
// Public so an application-owned custom section can take `"${VAR:-30}"` as a number too.
pub use settings::{optional_scalar_or_string, scalar_or_string};
pub use source::ConfigSource;
