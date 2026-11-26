//! Discovery primitives for non-page build inputs: migrations and seed files.

mod migration;
mod seed_file;

pub use migration::{Migration, discover_migrations};
pub use seed_file::discover_seed_files;
