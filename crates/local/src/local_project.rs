use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fundamental_core::address::TestApex;
use fundamental_manifest::{KeyPath, Manifest};
use serde::Deserialize;
use tracing::{info, instrument};

use crate::{ApexSet, ApexSource, DatabaseName, LocalError, LocalLogin, LocalOrigins, ProjectName};

/// What a project declares about its local origins: its name, its apexes and its origins.
#[derive(Debug, Clone)]
pub struct LocalProject {
    root: PathBuf,
    name: ProjectName,
    apexes: ApexSource,
    origins: LocalOrigins,
    databases: Vec<DatabaseName>,
    logins: Vec<LocalLogin>,
}

/// `[local]` as written.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalTable {
    #[serde(default)]
    apexes: Option<Vec<String>>,
    /// Each brand's development apex, by the brand's code, for a project whose apps wear brands.
    #[serde(default)]
    brands: Option<BTreeMap<String, String>>,
    #[serde(default)]
    origins: toml::Table,
    #[serde(default)]
    databases: Vec<String>,
    #[serde(default)]
    logins: BTreeMap<String, Vec<String>>,
}

impl LocalProject {
    /// Reads the project whose manifest is at or above `start`.
    ///
    /// # Errors
    ///
    /// [`LocalError::Manifest`] when no manifest is found or it cannot be read;
    /// [`LocalError::NoApexes`] (`LOCAL_008`) for a project with neither `[local]` nor `brands/`,
    /// decided first; [`LocalError::ProjectUnnamed`] (`LOCAL_014`), [`LocalError::ApexesDeclaredTwice`]
    /// (`LOCAL_007`), [`LocalError::ApexNotTest`] (`LOCAL_001`) and [`LocalError::OriginsInvalid`]
    /// (`LOCAL_010`) for what is declared wrongly.
    #[instrument(fields(start = %start.display()))]
    pub fn load(start: &Path) -> Result<Self, LocalError> {
        let manifest = Manifest::discover(start)?;
        Self::from_manifest(&manifest)
    }

    /// Reads the project `manifest` describes.
    ///
    /// # Errors
    ///
    /// As [`load`](Self::load), less finding the manifest.
    pub fn from_manifest(manifest: &Manifest) -> Result<Self, LocalError> {
        let root = manifest.root().to_path_buf();
        let declared: Option<LocalTable> = manifest.table(&KeyPath::from_segments(["local"]))?;
        let Some(table) = declared else {
            return Err(LocalError::NoApexes);
        };
        let name = match manifest.get(&KeyPath::from_segments(["project", "name"])) {
            Some(toml::Value::String(name)) => ProjectName::parse(name)?,
            Some(other) => {
                return Err(LocalError::ProjectUnnamed {
                    found: format!("a {}", other.type_str()),
                });
            }
            None => {
                return Err(LocalError::ProjectUnnamed {
                    found: "nothing".to_owned(),
                });
            }
        };
        let parse = |apex: &String| {
            TestApex::parse(apex).ok_or_else(|| LocalError::ApexNotTest { apex: apex.clone() })
        };
        let apexes = match (table.brands, table.apexes) {
            (Some(_), Some(_)) => return Err(LocalError::ApexesDeclaredTwice),
            (None, None) => return Err(LocalError::NoApexes),
            (None, Some(declared)) => {
                let parsed = declared.iter().map(parse).collect::<Result<Vec<_>, _>>()?;
                ApexSource::Declared(ApexSet::new(parsed).ok_or(LocalError::NoApexes)?)
            }
            (Some(brands), None) => {
                let by_brand: BTreeMap<String, TestApex> = brands
                    .iter()
                    .map(|(code, apex)| Ok((code.clone(), parse(apex)?)))
                    .collect::<Result<_, LocalError>>()?;
                let apexes =
                    ApexSet::new(by_brand.values().cloned()).ok_or(LocalError::NoApexes)?;
                ApexSource::Brands { by_brand, apexes }
            }
        };
        let origins = LocalOrigins::parse(&table.origins, manifest, &apexes)?;
        let databases = databases_of(&table.databases)?;
        let logins = logins_of(&table.logins)?;
        info!(project = %name, apexes = %apexes.apexes(), origins = origins.iter().count(), "local project read");
        Ok(Self {
            root,
            name,
            apexes,
            origins,
            databases,
            logins,
        })
    }

    /// The logins the project's apps connect to the local node as, with their roles, `[local.logins]`.
    #[must_use]
    pub fn logins(&self) -> &[LocalLogin] {
        &self.logins
    }

    /// The databases the project's apps read, which the machine's local node holds.
    #[must_use]
    pub fn databases(&self) -> &[DatabaseName] {
        &self.databases
    }

    /// The directory holding the project's `fun.toml`.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The project's name.
    #[must_use]
    pub const fn name(&self) -> &ProjectName {
        &self.name
    }

    /// Where the apexes come from, and what they are.
    #[must_use]
    pub const fn apex_source(&self) -> &ApexSource {
        &self.apexes
    }

    /// Every apex the project serves under.
    #[must_use]
    pub const fn apexes(&self) -> &ApexSet {
        self.apexes.apexes()
    }

    /// The origins the project's local edge serves, `[local.origins]`.
    #[must_use]
    pub const fn origins(&self) -> &LocalOrigins {
        &self.origins
    }
}

/// `[local.logins]`, each login and role a plain name.
fn logins_of(declared: &BTreeMap<String, Vec<String>>) -> Result<Vec<LocalLogin>, LocalError> {
    declared
        .iter()
        .map(|(name, roles)| LocalLogin::parse(name, roles))
        .collect()
}

/// `[local] databases`, each a plain name.
fn databases_of(declared: &[String]) -> Result<Vec<DatabaseName>, LocalError> {
    declared
        .iter()
        .map(|name| DatabaseName::parse(name))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use fundamental_core::Error as _;

    use super::LocalProject;

    fn project(manifest: &str) -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("a temporary directory");
        fs::write(root.path().join("fun.toml"), manifest).expect("a manifest");
        root
    }

    fn code(root: &Path) -> Option<&'static str> {
        LocalProject::load(root)
            .err()
            .map(|error| error.error_code())
    }

    #[test]
    fn a_project_names_the_databases_its_apps_read() {
        let root = project(
            "[project]\nname = \"acme\"\n\n[local]\napexes = [\"acme.test\"]\ndatabases = [\"ops\", \"www_reads\"]\n",
        );
        let local = LocalProject::load(root.path()).expect("read");
        let names: Vec<&str> = local.databases().iter().map(|name| name.as_str()).collect();
        assert_eq!(names, ["ops", "www_reads"]);
    }

    #[test]
    fn a_database_is_one_plain_lowercase_name() {
        for name in ["Ops", "ops;drop", "", "1ops", "o-ps", &"o".repeat(64)] {
            let root = project(&format!(
                "[project]\nname = \"acme\"\n\n[local]\napexes = [\"acme.test\"]\ndatabases = [\"{name}\"]\n"
            ));
            assert_eq!(code(root.path()), Some("LOCAL_024"), "{name}");
        }
    }

    #[test]
    fn a_project_names_the_logins_its_apps_connect_as_each_with_its_roles() {
        let root = project(
            "[project]\nname = \"acme\"\n\n[local]\napexes = [\"acme.test\"]\ndatabases = [\"ops\"]\n\n[local.logins]\nwww_site = [\"www\"]\n",
        );
        let local = LocalProject::load(root.path()).expect("read");
        let logins: Vec<(&str, Vec<&str>)> = local
            .logins()
            .iter()
            .map(|login| {
                (
                    login.name().as_str(),
                    login.roles().iter().map(|role| role.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(logins, [("www_site", vec!["www"])]);
    }

    #[test]
    fn a_login_and_its_roles_are_plain_names() {
        for logins in [
            "\"Www\" = [\"www\"]",
            "www_site = [\"www;drop\"]",
            "www_site = [\"\"]",
        ] {
            let root = project(&format!(
                "[project]\nname = \"acme\"\n\n[local]\napexes = [\"acme.test\"]\n\n[local.logins]\n{logins}\n"
            ));
            assert_eq!(code(root.path()), Some("LOCAL_026"), "{logins}");
        }
    }

    #[test]
    fn a_project_without_databases_names_none() {
        let root = project("[project]\nname = \"acme\"\n\n[local]\napexes = [\"acme.test\"]\n");
        assert!(
            LocalProject::load(root.path())
                .expect("read")
                .databases()
                .is_empty()
        );
    }

    #[test]
    fn an_apex_must_be_one_label_under_test() {
        for apex in ["acme.nl", "a.acme.test", "Acme.test", ".test"] {
            let root = project(&format!(
                "[project]\nname = \"acme\"\n\n[local]\napexes = [\"{apex}\"]\n"
            ));
            assert_eq!(code(root.path()), Some("LOCAL_001"), "{apex}");
        }
        let root = project(
            "[project]\nname = \"acme\"\n\n[local]\napexes = [\"acme.test\", \"acme-sandbox.test\"]\n",
        );
        let read = LocalProject::load(root.path()).expect("a valid project");
        assert_eq!(read.name().as_str(), "acme");
        assert_eq!(
            read.apexes().to_string(),
            "`acme-sandbox.test`, `acme.test`"
        );
    }

    #[test]
    fn brands_and_declared_apexes_together_are_refused() {
        let root = project(
            "[project]\nname = \"example\"\n\n[local]\napexes = [\"example.test\"]\n\n[local.brands]\nexample = \"example.test\"\n",
        );
        assert_eq!(code(root.path()), Some("LOCAL_007"));
    }

    #[test]
    fn a_project_with_no_apex_or_no_name_is_refused() {
        assert_eq!(
            code(project("[project]\nname = \"x\"\n").path()),
            Some("LOCAL_008")
        );
        assert_eq!(
            code(project("[project]\nname = \"x\"\n\n[local]\napexes = []\n").path()),
            Some("LOCAL_008")
        );
        let apexes = "\n[local]\napexes = [\"x.test\"]\n";
        assert_eq!(
            code(project(&format!("[project]\n{apexes}")).path()),
            Some("LOCAL_014")
        );
        assert_eq!(
            code(project(&format!("[project]\nname = \"../x\"\n{apexes}")).path()),
            Some("LOCAL_014")
        );
    }

    #[test]
    fn a_project_that_does_not_take_part_is_not_judged_on_its_name() {
        for manifest in [
            "[project]\nname = \"my_app\"\n",
            "[apps.www.web]\nport = 3000\n",
        ] {
            assert_eq!(
                code(project(manifest).path()),
                Some("LOCAL_008"),
                "{manifest}"
            );
        }
    }

    #[test]
    fn a_brand_projects_apexes_are_its_brands() {
        let root = project(
            "[project]\nname = \"example\"\n\n[local.brands]\nexample = \"example.test\"\nejemplo = \"ejemplo.test\"\n",
        );
        let read = LocalProject::load(root.path()).expect("a brand project");
        assert_eq!(read.apexes().to_string(), "`ejemplo.test`, `example.test`");
        assert_eq!(
            read.apex_source()
                .brand("ejemplo")
                .map(|apex| apex.as_str()),
            Some("ejemplo.test")
        );
    }

    #[test]
    fn a_brands_local_name_must_be_one_label_under_test() {
        let root =
            project("[project]\nname = \"example\"\n\n[local.brands]\nexample = \"example.com\"\n");
        assert_eq!(code(root.path()), Some("LOCAL_001"));
    }
}
