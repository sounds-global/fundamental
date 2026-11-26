//! [`ProjectConfig`], from the `[project]` section.

use super::ProjectType;
use serde::{Deserialize, Serialize};

/// Project metadata from the `[project]` section.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Project name.
    #[serde(default)]
    pub name: String,
    /// Project version.
    #[serde(default)]
    pub version: String,
    /// Project type - determines config shape.
    #[serde(default, rename = "type")]
    pub project_type: ProjectType,
}
