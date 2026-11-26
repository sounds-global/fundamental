//! Scaffolding shared by `fun new` and `fun add app`: name, prompts, and flag/prompt/default resolution.

mod project_name;
mod project_plan;
mod prompts;
mod resolve;

pub(crate) use project_name::ProjectName;
pub(crate) use project_plan::{DEFAULT_SOCKET_PORT, ProjectPlan};
pub(crate) use prompts::ScaffoldPrompts;
pub(crate) use resolve::PlanResolver;
