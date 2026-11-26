//! A Fundamental project's settings, read through one loader.
//!
//! [`Manifest`] reads `fun.toml`, with `fun.<environment>.toml` (or, in development, the
//! uncommitted `fun.local.toml`) over it, into one checked view whose values remember their
//! file and line.
//!
//! ```
//! use fundamental_manifest::{KeyPath, Manifest};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let project = std::env::temp_dir().join(format!("manifest-crate-doc-{}", std::process::id()));
//! # std::fs::create_dir_all(&project)?;
//! # std::fs::write(project.join("fun.toml"), "[project]\nname = \"acme\"\n\n[apps.customer.web]\nport = 3000\n")?;
//! # std::fs::write(project.join("fun.production.toml"), "[apps.customer.web]\nport = 8080\n")?;
//! let production = Manifest::load_for(&project, "production")?;
//! let port: KeyPath = "apps.customer.web.port".parse()?;
//! assert_eq!(production.get(&port), Some(&toml::Value::Integer(8080)));
//! assert_eq!(production.origins(&port)[0].to_string(), "fun.production.toml:2:1");
//! # std::fs::remove_dir_all(&project)?;
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod app_key;
mod app_listeners;
#[cfg(feature = "edit")]
mod edit_lock;
mod files;
mod key_path;
mod leaf;
mod manifest;
#[cfg(feature = "edit")]
mod manifest_editor;
mod manifest_error;
mod manifest_finding;
mod origin;
mod parsed_file;
#[cfg(feature = "ledger")]
pub mod pending;
mod source_file;

pub use app_key::AppKey;
pub use app_listeners::APP_LISTENERS;
pub use key_path::KeyPath;
pub use manifest::Manifest;
#[cfg(feature = "edit")]
pub use manifest_editor::ManifestEditor;
pub use manifest_error::ManifestError;
pub use manifest_finding::ManifestFinding;
pub use origin::Origin;
