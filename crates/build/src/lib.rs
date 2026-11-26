//! Framework-neutral build engine for the [Fundamental](https://docs.rs/fundamental) framework.
//!
//! It turns an application's directory layout into route IR and generated source, and links
//! no Leptos and no axum:
//!
//! - [`RouteTree`] scans `pages/` into sorted [`Route`]s and their [`Layout`] chains.
//! - [`render_link_builders`] and [`render_page_slugs`] emit typed link builders and slug tables.
//! - [`PagesBuild`] is the one call a web app's `build.rs` makes; [`RoutesBuild`] is a routes
//!   crate's.
//! - [`discover_migrations`], [`discover_seed_files`] and [`check_app_layout`] find and check
//!   build inputs.
//!
//! ```rust
//! # extern crate fundamental_build;
//! use fundamental_build::{RouteTree, render_link_builders};
//!
//! # fn run(pages_dir: &std::path::Path) -> Result<(), fundamental_build::BuildError> {
//! let tree = RouteTree::scan(pages_dir)?;
//! let link_source = render_link_builders(&tree);
//! // write `link_source` to OUT_DIR and `include!` it from your crate.
//! # let _ = link_source;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod architecture;
mod build_error;
mod bundle;
mod consistency;
mod discover;
mod hydration;
mod pages_build;
mod route;
mod routes_build;
pub mod tailwind;
mod wasm;

pub use architecture::{ArchitectureFinding, Ring};
pub use build_error::BuildError;
pub use bundle::{BundleFingerprint, DistSnapshot, StepPrint, StepRecord};
pub use consistency::check_app_layout;
pub use discover::{Migration, discover_migrations, discover_seed_files};
pub use hydration::Hydration;
pub use pages_build::PagesBuild;
pub use route::{Layout, Route, RouteKind, RouteTree, render_link_builders, render_page_slugs};
pub use routes_build::RoutesBuild;
pub use wasm::{IslandsCrate, WasmArtifacts, WasmBuild, bindgen_cli_version, locked_bindgen_cli};
