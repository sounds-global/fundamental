use std::fmt;

use fundamental_core::address::DnsLabel;

use crate::LocalError;

/// A project's name on this machine: the manifest's `[project] name`, one lower-case DNS label.
///
/// Being a label keeps it from naming a path outside the registry's files.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectName(DnsLabel);

impl ProjectName {
    /// Parses a project name.
    ///
    /// # Errors
    ///
    /// [`LocalError::ProjectUnnamed`] (`LOCAL_014`) when `text` is not one lower-case DNS label.
    pub fn parse(text: &str) -> Result<Self, LocalError> {
        DnsLabel::parse(text)
            .map(Self)
            .ok_or_else(|| LocalError::ProjectUnnamed {
                found: format!("`{text}`"),
            })
    }

    /// The name as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for ProjectName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::ProjectName;

    #[test]
    fn a_project_name_is_one_label_and_never_a_path() {
        assert_eq!(
            ProjectName::parse("acme").map(|name| name.to_string()).ok(),
            Some("acme".to_owned())
        );
        for text in ["", "../x", "Acme", "a/b", "a.b"] {
            let refused = ProjectName::parse(text).map(|name| name.to_string());
            assert_eq!(
                refused.map_err(|error| error.error_code()),
                Err("LOCAL_014"),
                "{text}"
            );
        }
    }
}
