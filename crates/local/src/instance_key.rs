use std::fmt;

use crate::{InstanceRole, ProjectName};

/// One instance on this machine: a project, and which of its checkouts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceKey {
    project: ProjectName,
    role: InstanceRole,
}

impl InstanceKey {
    /// The instance of `project` in `role`.
    #[must_use]
    pub const fn new(project: ProjectName, role: InstanceRole) -> Self {
        Self { project, role }
    }

    /// The project.
    #[must_use]
    pub const fn project(&self) -> &ProjectName {
        &self.project
    }

    /// Main or a lane.
    #[must_use]
    pub const fn role(&self) -> &InstanceRole {
        &self.role
    }

    /// The name this instance's files are kept under: `main`, or `lane-<label>`.
    #[must_use]
    pub fn slot(&self) -> String {
        match self.role.label() {
            None => "main".to_owned(),
            Some(label) => format!("lane-{label}"),
        }
    }
}

impl fmt::Display for InstanceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.project, self.role)
    }
}
