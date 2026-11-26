use std::fmt;

use fundamental_core::address::DnsLabel;

use crate::LocalError;

/// A lane's label: the one DNS label its names carry in front of `lane.<apex>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceLabel(DnsLabel);

impl InstanceLabel {
    /// Parses a label exactly as given; upper case is refused, never folded.
    ///
    /// # Errors
    ///
    /// [`LocalError::LabelInvalid`] (`LOCAL_019`) when `text` is not one lower-case DNS label.
    pub fn parse(text: &str) -> Result<Self, LocalError> {
        DnsLabel::parse(text)
            .map(Self)
            .ok_or_else(|| LocalError::LabelInvalid {
                label: text.to_owned(),
            })
    }

    /// The label a lane on `branch` takes: lower-cased, each run of other characters one hyphen
    /// (`Fix-Login` is `fix-login`); the one place a spelling is folded.
    ///
    /// # Errors
    ///
    /// [`LocalError::LabelInvalid`] (`LOCAL_019`) when nothing of the branch survives as a label.
    pub fn from_branch(branch: &str) -> Result<Self, LocalError> {
        let mut label = String::with_capacity(branch.len());
        for character in branch.chars().flat_map(char::to_lowercase) {
            if character.is_ascii_lowercase() || character.is_ascii_digit() {
                label.push(character);
            } else if !label.ends_with('-') {
                label.push('-');
            }
        }
        let label = label.trim_matches('-');
        Self::parse(label).map_err(|_| LocalError::LabelInvalid {
            label: branch.to_owned(),
        })
    }

    /// The label as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// The label as the DNS label it is.
    #[must_use]
    pub const fn dns(&self) -> &DnsLabel {
        &self.0
    }
}

impl fmt::Display for InstanceLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::InstanceLabel;

    #[test]
    fn a_branch_becomes_its_lanes_label() {
        for (branch, label) in [
            ("Fix-Login", "fix-login"),
            ("feature/142#S3", "feature-142-s3"),
            ("feature/Ünïcode", "feature-n-code"),
            ("ci", "ci"),
        ] {
            let derived = InstanceLabel::from_branch(branch).map(|label| label.to_string());
            assert_eq!(derived.ok().as_deref(), Some(label), "{branch}");
        }
    }

    #[test]
    fn a_label_is_parsed_exactly_and_an_empty_one_is_refused() {
        for text in ["Lane", "a.b", "", "-"] {
            let refused = InstanceLabel::parse(text).map(|label| label.to_string());
            assert_eq!(
                refused.map_err(|error| error.error_code()),
                Err("LOCAL_019")
            );
        }
        let empty = InstanceLabel::from_branch("---").map(|label| label.to_string());
        assert_eq!(empty.map_err(|error| error.error_code()), Err("LOCAL_019"));
        let long = InstanceLabel::from_branch(&"a".repeat(64)).map(|label| label.to_string());
        assert_eq!(long.map_err(|error| error.error_code()), Err("LOCAL_019"));
    }
}
