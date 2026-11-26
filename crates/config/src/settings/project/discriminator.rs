//! [`ProjectType`], selecting the overall config shape.

use serde::{Deserialize, Serialize};

/// Project type: a service project (the default) or a database-only domain project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectType {
    /// Fundamental service project.
    #[default]
    Service,
    /// Domain project: owns models and migrations, no deployed surfaces.
    Domain,
}

impl std::fmt::Display for ProjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Service => write!(f, "service"),
            Self::Domain => write!(f, "domain"),
        }
    }
}
