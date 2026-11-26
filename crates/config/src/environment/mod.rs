//! Deciding which environment a process runs as, and so which rules it is held to.

mod environment_origin;
mod resolve_environment;
mod resolved_environment;

pub use environment_origin::EnvironmentOrigin;
pub use resolve_environment::resolve_environment;
pub use resolved_environment::ResolvedEnvironment;
