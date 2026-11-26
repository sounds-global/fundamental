//! [`Lints`]: the level of every lint, and the project facts some lints need.

use std::collections::BTreeMap;
use std::path::Path;

use fundamental_core::model::sql::{Level, Lint};
use fundamental_manifest::{KeyPath, Manifest, ManifestError};

use super::LintsError;

/// The levels a project holds its migrations' lints to, with its tenant column and whether
/// its database is colocated. [`Lints::read`] reads them over the defaults from `fun.toml`:
///
/// ```toml
/// [migrations]
/// colocated = true            # the database was created WITH COLOCATION = true
/// tenant = "organisation_id"  # turns on tenant_not_leading
///
/// [migrations.lints]
/// hot_shard = "refuse"        # refuse | warn | allow
/// unindexed_reference = "allow"
/// ```
///
/// The levels are the same in every environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lints {
    levels: BTreeMap<Lint, Level>,
    tenant: Option<String>,
    colocated: bool,
}

impl Lints {
    /// The level `lint` is held to.
    #[must_use]
    pub fn level(&self, lint: Lint) -> Level {
        self.levels
            .get(&lint)
            .copied()
            .unwrap_or_else(|| lint.default_level())
    }

    /// It with `lint` held to `level`.
    #[must_use]
    pub fn with(mut self, lint: Lint, level: Level) -> Self {
        self.levels.insert(lint, level);
        self
    }

    /// It with `column` the project's tenant.
    #[must_use]
    pub fn tenant(mut self, column: impl Into<String>) -> Self {
        self.tenant = Some(column.into());
        self
    }

    /// It for a database created `WITH COLOCATION = true`, or not.
    #[must_use]
    pub const fn colocated(mut self, colocated: bool) -> Self {
        self.colocated = colocated;
        self
    }

    /// The project's tenant column, when it names one.
    #[must_use]
    pub fn tenant_column(&self) -> Option<&str> {
        self.tenant.as_deref()
    }

    /// Whether the project's database is colocated.
    #[must_use]
    pub const fn is_colocated(&self) -> bool {
        self.colocated
    }

    /// The lints of the project at or above `root`, read from its `fun.toml` at run time; a
    /// migrator compiles them in with [`lints!`](crate::migration::lints) instead.
    ///
    /// # Errors
    /// A [`LintsError`] naming the file and line for an unreadable file, an unknown lint or
    /// level, a misshapen setting, or a `[migrations]` setting outside `fun.toml`.
    pub fn read(root: impl AsRef<Path>) -> Result<Self, LintsError> {
        let manifest = match Manifest::find_root(root.as_ref()) {
            Ok(project) => Manifest::load(project),
            Err(error) => Err(error),
        };
        let manifest = match manifest {
            Ok(manifest) => manifest,
            Err(ManifestError::NotFound { .. }) => return Ok(Self::default()),
            Err(error) => {
                return Err(LintsError::Unreadable {
                    detail: error.to_string(),
                });
            }
        };
        let migrations = KeyPath::from_segments(["migrations"]);
        if let Some(beside) = manifest
            .origins(&migrations)
            .into_iter()
            .find(|origin| origin.file() != Path::new(Manifest::FILE))
        {
            return Err(LintsError::NotInFunToml {
                at: beside.to_string(),
            });
        }
        Self::of(manifest.as_table(), |key| {
            manifest
                .origins(key)
                .first()
                .map_or_else(|| Manifest::FILE.to_owned(), ToString::to_string)
        })
    }

    /// The lints the text of a `fun.toml` sets: what [`lints!`](crate::migration::lints) compiles
    /// into a migrator.
    ///
    /// # Errors
    /// As [`read`](Self::read), naming `fun.toml` without a line.
    pub fn from_toml(text: &str) -> Result<Self, LintsError> {
        let table = text
            .parse::<toml::Table>()
            .map_err(|error| LintsError::Unreadable {
                detail: format!("fun.toml: {}", error.message()),
            })?;
        Self::of(&table, |_| Manifest::FILE.to_owned())
    }

    fn of(settings: &toml::Table, at: impl Fn(&KeyPath) -> String) -> Result<Self, LintsError> {
        let mut lints = Self::default();
        let migrations = KeyPath::from_segments(["migrations"]);
        let Some(section) = settings.get("migrations") else {
            return Ok(lints);
        };
        let section = section.as_table().ok_or_else(|| LintsError::Shape {
            at: at(&migrations),
            key: "migrations".to_owned(),
            expected: "a table".to_owned(),
        })?;
        let colocated = migrations.child("colocated");
        if let Some(value) = section.get("colocated") {
            lints.colocated = value.as_bool().ok_or_else(|| LintsError::Shape {
                at: at(&colocated),
                key: "colocated".to_owned(),
                expected: "true or false".to_owned(),
            })?;
        }
        let tenant = migrations.child("tenant");
        if let Some(value) = section.get("tenant") {
            let column = value
                .as_str()
                .filter(|column| is_identifier(column))
                .ok_or_else(|| LintsError::Shape {
                    at: at(&tenant),
                    key: "tenant".to_owned(),
                    expected: "a lowercase column name".to_owned(),
                })?;
            lints.tenant = Some(column.to_owned());
        }
        let levels = migrations.child("lints");
        let Some(table) = section.get("lints") else {
            return Ok(lints);
        };
        let table = table.as_table().ok_or_else(|| LintsError::Shape {
            at: at(&levels),
            key: "lints".to_owned(),
            expected: "a table of lint = \"refuse\" | \"warn\" | \"allow\"".to_owned(),
        })?;
        for (name, value) in table {
            let key = levels.child(name);
            let lint = Lint::parse(name).ok_or_else(|| LintsError::UnknownLint {
                at: at(&key),
                name: name.clone(),
            })?;
            let level =
                value
                    .as_str()
                    .and_then(Level::parse)
                    .ok_or_else(|| LintsError::UnknownLevel {
                        at: at(&key),
                        lint: name.clone(),
                        level: value.to_string(),
                    })?;
            lints.levels.insert(lint, level);
        }
        Ok(lints)
    }
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use fundamental_core::model::sql::{Level, Lint};

    use super::Lints;
    use crate::migration::lint::LintsError;

    fn project(settings: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a directory");
        std::fs::write(
            dir.path().join("fun.toml"),
            format!("[project]\nname = \"acme\"\n\n{settings}"),
        )
        .expect("written");
        std::fs::create_dir_all(dir.path().join("crates/orders")).expect("made");
        dir
    }

    #[test]
    fn a_project_that_sets_nothing_holds_every_lint_to_its_default() {
        let dir = project("");

        let lints = Lints::read(dir.path().join("crates/orders")).expect("read");

        assert_eq!(lints, Lints::default());
        for lint in Lint::ALL {
            assert_eq!(lints.level(lint), lint.default_level());
        }
        assert!(!lints.is_colocated());
        assert_eq!(lints.tenant_column(), None);
    }

    #[test]
    fn the_nearest_fun_toml_sets_levels_the_tenant_and_colocation() {
        let dir = project(
            "[migrations]\ncolocated = true\ntenant = \"organisation_id\"\n\n\
             [migrations.lints]\nhot_shard = \"refuse\"\nbackfill = \"allow\"\n",
        );

        let lints = Lints::read(dir.path().join("crates/orders")).expect("read");

        assert_eq!(lints.level(Lint::HotShard), Level::Refuse);
        assert_eq!(lints.level(Lint::Backfill), Level::Allow);
        assert_eq!(lints.level(Lint::Destructive), Level::Warn);
        assert!(lints.is_colocated());
        assert_eq!(lints.tenant_column(), Some("organisation_id"));
    }

    #[test]
    fn a_lint_no_one_has_is_refused_at_its_line() {
        let dir = project("[migrations.lints]\nhot-shard = \"refuse\"\n");

        let error = Lints::read(dir.path()).expect_err("refused");

        let LintsError::UnknownLint { at, name } = &error else {
            panic!("expected UnknownLint, got {error:?}");
        };
        assert_eq!(name, "hot-shard");
        assert!(at.starts_with("fun.toml:5:"), "{at}");
    }

    #[test]
    fn a_level_no_lint_has_is_refused_at_its_line() {
        let dir = project("[migrations.lints]\nhot_shard = \"error\"\n");

        let error = Lints::read(dir.path()).expect_err("refused");

        assert!(
            matches!(&error, LintsError::UnknownLevel { lint, at, .. }
                if lint == "hot_shard" && at.starts_with("fun.toml:5:")),
            "{error:?}"
        );
    }

    #[test]
    fn a_colocation_that_is_not_a_flag_is_refused() {
        let dir = project("[migrations]\ncolocated = \"yes\"\n");

        let error = Lints::read(dir.path()).expect_err("refused");

        assert!(matches!(error, LintsError::Shape { .. }), "{error:?}");
    }

    #[test]
    fn no_fun_toml_at_all_is_the_defaults() {
        let dir = tempfile::tempdir().expect("a directory");

        assert_eq!(Lints::read(dir.path()).expect("read"), Lints::default());
    }

    #[test]
    fn the_text_of_a_fun_toml_sets_levels_the_tenant_and_colocation() {
        let lints = Lints::from_toml(
            "[project]\nname = \"acme\"\n\n[migrations]\ncolocated = true\n\
             tenant = \"organisation_id\"\n\n[migrations.lints]\nhot_shard = \"refuse\"\n",
        )
        .expect("read");

        assert_eq!(
            lints,
            Lints::default()
                .with(Lint::HotShard, Level::Refuse)
                .tenant("organisation_id")
                .colocated(true)
        );
        assert_eq!(
            Lints::from_toml("[project]\nname = \"acme\"\n").expect("read"),
            Lints::default()
        );
    }

    #[test]
    fn a_lint_no_one_has_in_the_text_is_refused() {
        let error =
            Lints::from_toml("[migrations.lints]\nhot-shard = \"refuse\"\n").expect_err("refused");

        assert!(
            matches!(&error, LintsError::UnknownLint { name, at } if name == "hot-shard" && at == "fun.toml"),
            "{error:?}"
        );
        assert!(matches!(
            Lints::from_toml("[migrations").expect_err("refused"),
            LintsError::Unreadable { .. }
        ));
    }

    #[test]
    fn a_level_set_beside_fun_toml_is_refused_since_levels_are_the_same_everywhere() {
        let dir = project("");
        std::fs::write(
            dir.path().join("fun.local.toml"),
            "[migrations.lints]\nhot_shard = \"allow\"\n",
        )
        .expect("written");

        let error = Lints::read(dir.path()).expect_err("refused");

        assert!(
            matches!(&error, LintsError::NotInFunToml { at } if at.starts_with("fun.local.toml:")),
            "{error:?}"
        );
    }

    #[test]
    fn a_level_set_in_code_wins_over_the_default() {
        let lints = Lints::default().with(Lint::Destructive, Level::Allow);

        assert_eq!(lints.level(Lint::Destructive), Level::Allow);
        assert_eq!(lints.level(Lint::HotShard), Level::Warn);
    }
}
