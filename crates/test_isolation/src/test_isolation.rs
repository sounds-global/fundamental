//! [`TestIsolation`], the check that refuses a test reaching beyond its own rows.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::function_scan::{Found, Names, scan};
use crate::schema_change::schema_changes;
use crate::{IsolationError, IsolationFinding, IsolationRule, RunAlone};

/// The isolation check, configured with what a project runs alone and the calls it judges.
#[derive(Debug, Clone)]
pub struct TestIsolation {
    run_alone: RunAlone,
    names: Names,
    relays: BTreeSet<(PathBuf, String)>,
}

impl TestIsolation {
    /// The check for a project that runs `run_alone` alone.
    #[must_use]
    pub fn new(run_alone: RunAlone) -> Self {
        Self {
            run_alone,
            names: Names {
                leasing: BTreeSet::new(),
                revoking: ["close".to_owned()].into(),
                drains: ["JobsDrain::new".to_owned()].into(),
                confining: ["confined_to".to_owned()].into(),
                race_windows: BTreeSet::new(),
            },
            relays: BTreeSet::new(),
        }
    }

    /// Also refuses the calls `names` as leases.
    #[must_use]
    pub fn leasing<'a>(mut self, names: impl IntoIterator<Item = &'a str>) -> Self {
        self.names
            .leasing
            .extend(names.into_iter().map(str::to_owned));
        self
    }

    /// Also counts a call to one of `names` as revoking a lease, when called on the lease's binding.
    #[must_use]
    pub fn revoking<'a>(mut self, names: impl IntoIterator<Item = &'a str>) -> Self {
        self.names
            .revoking
            .extend(names.into_iter().map(str::to_owned));
        self
    }

    /// Also judges the calls `names` as drains or sweeps.
    #[must_use]
    pub fn drains<'a>(mut self, names: impl IntoIterator<Item = &'a str>) -> Self {
        self.names
            .drains
            .extend(names.into_iter().map(str::to_owned));
        self
    }

    /// Also counts a drain inside a call to one of `names` as confined.
    #[must_use]
    pub fn confining<'a>(mut self, names: impl IntoIterator<Item = &'a str>) -> Self {
        self.names
            .confining
            .extend(names.into_iter().map(str::to_owned));
        self
    }

    /// Admits a sleep for one of the constants `names`, the windows race tests hold open.
    #[must_use]
    pub fn race_windows<'a>(mut self, names: impl IntoIterator<Item = &'a str>) -> Self {
        self.names
            .race_windows
            .extend(names.into_iter().map(str::to_owned));
        self
    }

    /// Admits an unconfined drain in each `(file, function)` of `relays`, `file` matched as a path suffix.
    #[must_use]
    pub fn relays<'a>(mut self, relays: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        self.relays.extend(
            relays
                .into_iter()
                .map(|(file, function)| (PathBuf::from(file), function.to_owned())),
        );
        self
    }

    /// Every place the test source `source`, at `file`, reaches beyond its own rows.
    ///
    /// # Errors
    ///
    /// [`IsolationError::Unparsable`] for a source that is not Rust, and
    /// [`IsolationError::Io`] for a fixture that cannot be read.
    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub fn check(
        &self,
        file: &Path,
        source: &str,
    ) -> Result<Vec<IsolationFinding>, IsolationError> {
        let syntax = syn::parse_file(source).map_err(|error| IsolationError::Unparsable {
            file: file.to_path_buf(),
            message: error.to_string(),
        })?;
        let binary = binary_of(file);
        let scanned = scan(&syntax, &self.names);
        let mut findings = Vec::new();
        for site in scanned.sites {
            let alone =
                self.run_alone.holds_test(&site.function) || self.run_alone.holds_binary(&binary);
            let finding = |rule, detail: String| IsolationFinding {
                rule,
                file: file.to_path_buf(),
                line: site.line,
                function: site.function.clone(),
                detail,
            };
            match &site.found {
                Found::Lease { call, bound } => {
                    if !alone {
                        findings.push(finding(
                            IsolationRule::LeaseBesideOthers,
                            format!("`{call}` leases a role while other tests run"),
                        ));
                    }
                    let revoked = bound.iter().any(|name| {
                        let in_body =
                            |name: &String| scanned.revoked.contains(&(site.body, name.clone()));
                        in_body(name)
                            || scanned.aliases.iter().any(|(body, alias, of)| {
                                *body == site.body && of == name && in_body(alias)
                            })
                    });
                    if !revoked {
                        findings.push(finding(
                            IsolationRule::LeaseNotRevoked,
                            format!("what `{call}` leased is never closed"),
                        ));
                    }
                }
                Found::Drain { call, confined } => {
                    let relay = self
                        .relays
                        .iter()
                        .any(|(path, function)| *function == site.function && file.ends_with(path));
                    if !confined && !alone && !relay {
                        findings.push(finding(
                            IsolationRule::UnconfinedDrain,
                            format!("`{call}` fires every due job, other tests' included"),
                        ));
                    }
                }
                Found::Sleep { call } => {
                    if !alone {
                        findings.push(finding(
                            IsolationRule::FixedSleep,
                            format!("`{call}` waits a fixed time"),
                        ));
                    }
                }
                Found::DiscardedWait => findings.push(finding(
                    IsolationRule::DiscardedWait,
                    "what `eventually` answered is dropped".to_owned(),
                )),
                Found::Fixture { path } => {
                    if alone {
                        continue;
                    }
                    let fixture = file.parent().unwrap_or(Path::new("")).join(path);
                    let script =
                        std::fs::read_to_string(&fixture).map_err(|source| IsolationError::Io {
                            path: fixture.clone(),
                            source,
                        })?;
                    let changes = schema_changes(&script);
                    if !changes.is_empty() {
                        findings.push(finding(
                            IsolationRule::SchemaChange,
                            format!("`{path}` runs {}", changes.join(", ")),
                        ));
                    }
                }
            }
        }
        Ok(findings)
    }
}

/// The test binary `file` belongs to: its first path segment under `tests/`.
fn binary_of(file: &Path) -> String {
    let path = file.to_string_lossy().replace('\\', "/");
    let under = path
        .rsplit_once("tests/")
        .map_or(path.as_str(), |(_, under)| under);
    // In a crate's one test binary, `tests/it/`, a module stands where a file's binary stood.
    let mut segments = under.split('/');
    let first = segments.next().unwrap_or_default();
    let named = match segments.next() {
        Some(module) if first == ONE_BINARY => module,
        _ => first,
    };
    named.trim_end_matches(".rs").to_owned()
}

/// The directory of a crate's one integration-test binary, `tests/it/main.rs`.
const ONE_BINARY: &str = "it";

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::binary_of;

    #[test]
    fn a_test_file_is_its_own_binary_and_a_module_directory_names_one() {
        assert_eq!(
            binary_of(Path::new("apps/mijn/tests/a_deposit_is_paid.rs")),
            "a_deposit_is_paid"
        );
        assert_eq!(
            binary_of(Path::new("crates/gates/tests/gates/main.rs")),
            "gates"
        );
        assert_eq!(
            binary_of(Path::new("apps/mijn/tests/support/billing.rs")),
            "support"
        );
    }

    #[test]
    fn a_module_of_the_one_test_binary_is_named_as_its_file_was() {
        assert_eq!(
            binary_of(Path::new("apps/mijn/tests/it/a_deposit_is_paid.rs")),
            "a_deposit_is_paid"
        );
        assert_eq!(
            binary_of(Path::new("apps/mijn/tests/it/support/billing.rs")),
            "support"
        );
    }
}
