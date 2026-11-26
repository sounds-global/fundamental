//! Page source checks plus optional, explicit crate dependency rules.

mod architecture_check;
mod architecture_finding;
mod architecture_table;
mod dependency;
mod file_rule;
mod member;
mod ring;
mod verdict;
mod view_scan;
mod workspace_graph;

pub(crate) use architecture_check::ArchitectureCheck;
pub use architecture_finding::ArchitectureFinding;
pub use ring::Ring;
