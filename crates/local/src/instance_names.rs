use fundamental_core::address::{LocalSuffix, TestApex};

use crate::{Checkout, InstanceKey, InstanceRole, LocalProject};

/// Who an instance is and the names it serves, from the manifest and checkout alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceNames {
    /// The project and which of its checkouts.
    pub key: InstanceKey,
    /// Each apex, with the suffix this instance's names end in under it.
    pub suffixes: Vec<(TestApex, LocalSuffix)>,
}

impl InstanceNames {
    /// The names `checkout` serves for `project`.
    #[must_use]
    pub fn new(project: &LocalProject, checkout: &Checkout) -> Self {
        Self::of(
            InstanceKey::new(project.name().clone(), checkout.role().clone()),
            project,
        )
    }

    /// The names the instance `key` serves under each of `project`'s apexes.
    #[must_use]
    pub fn of(key: InstanceKey, project: &LocalProject) -> Self {
        let suffixes = project
            .apexes()
            .iter()
            .map(|apex| (apex.clone(), key.role().suffix(apex)))
            .collect();
        Self { key, suffixes }
    }

    /// The names as environment variables, in a fixed order; `acme-sandbox.test` gives
    /// `FUN_LOCAL_SUFFIX_ACME_SANDBOX`.
    #[must_use]
    pub fn variables(&self) -> Vec<(String, String)> {
        let mut variables = vec![
            (
                "FUN_LOCAL_PROJECT".to_owned(),
                self.key.project().to_string(),
            ),
            (
                "FUN_LOCAL_ROLE".to_owned(),
                match self.key.role() {
                    InstanceRole::Main => "main".to_owned(),
                    InstanceRole::Lane(_) => "lane".to_owned(),
                },
            ),
            (
                "FUN_LOCAL_LABEL".to_owned(),
                self.key
                    .role()
                    .label()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            ),
        ];
        variables.extend(self.suffixes.iter().map(|(apex, suffix)| {
            (
                format!(
                    "FUN_LOCAL_SUFFIX_{}",
                    apex.label().to_uppercase().replace('-', "_")
                ),
                suffix.to_string(),
            )
        }));
        variables
    }

    /// The names as `NAME='value'` lines a POSIX shell can `eval`.
    #[must_use]
    pub fn shell(&self) -> String {
        shell_lines(&self.variables())
    }
}

/// `NAME='value'` lines, each value single-quoted so a shell `eval` takes it literally.
pub(crate) fn shell_lines(variables: &[(String, String)]) -> String {
    variables
        .iter()
        .map(|(name, value)| format!("{name}='{}'\n", value.replace('\'', r"'\''")))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::InstanceLabel;

    fn project(root: &Path) -> LocalProject {
        fs::write(
            root.join("fun.toml"),
            "[project]\nname = \"acme\"\n\n[local]\napexes = [\"acme.test\", \"acme-sandbox.test\"]\n",
        )
        .expect("a manifest");
        LocalProject::load(root).expect("a project")
    }

    #[test]
    fn a_lane_is_named_under_each_apexes_lanes_twin_without_a_claim() {
        let dir = tempfile::tempdir().expect("a directory");
        let project = project(dir.path());
        let checkout = Checkout::new(
            dir.path().to_path_buf(),
            InstanceRole::Lane(InstanceLabel::parse("fix-login").expect("a label")),
        );
        let shell = InstanceNames::new(&project, &checkout).shell();
        assert_eq!(
            shell,
            "FUN_LOCAL_PROJECT='acme'\n\
             FUN_LOCAL_ROLE='lane'\n\
             FUN_LOCAL_LABEL='fix-login'\n\
             FUN_LOCAL_SUFFIX_ACME_SANDBOX='fix-login.acme-sandbox-lane.test'\n\
             FUN_LOCAL_SUFFIX_ACME='fix-login.acme-lane.test'\n"
        );
    }

    #[test]
    fn main_is_named_at_the_apexes_with_an_empty_label() {
        let dir = tempfile::tempdir().expect("a directory");
        let project = project(dir.path());
        let checkout = Checkout::new(dir.path().to_path_buf(), InstanceRole::Main);
        let variables = InstanceNames::new(&project, &checkout).variables();
        assert!(variables.contains(&("FUN_LOCAL_ROLE".to_owned(), "main".to_owned())));
        assert!(variables.contains(&("FUN_LOCAL_LABEL".to_owned(), String::new())));
        assert!(variables.contains(&(
            "FUN_LOCAL_SUFFIX_ACME_SANDBOX".to_owned(),
            "acme-sandbox.test".to_owned()
        )));
    }

    #[test]
    fn a_quote_in_a_value_cannot_end_the_shell_word() {
        let lines = shell_lines(&[("A".to_owned(), "it's".to_owned())]);
        assert_eq!(lines, "A='it'\\''s'\n");
    }
}
