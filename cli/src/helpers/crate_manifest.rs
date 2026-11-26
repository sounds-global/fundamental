//! The [`CrateManifest`]: a format-preserving editor for a member crate's `Cargo.toml`.

use std::fs;
use std::path::{Path, PathBuf};

use toml_edit::{Array, DocumentMut, Item, Table, Value, value};
use tracing::{debug, instrument};

use crate::CliError;

/// A loaded, editable member-crate `Cargo.toml`, keeping comments, order and whitespace.
#[derive(Debug)]
pub(crate) struct CrateManifest {
    path: PathBuf,
    doc: DocumentMut,
}

impl CrateManifest {
    /// Load and parse the `Cargo.toml` at `path`.
    ///
    /// # Errors
    /// [`CliError::Io`] if it cannot be read, [`CliError::TomlParse`] if it is not valid TOML.
    #[instrument(skip_all, fields(path = %path.display()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) fn load(path: &Path) -> Result<Self, CliError> {
        let text = fs::read_to_string(path).map_err(|e| CliError::Io {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
        let doc = text
            .parse::<DocumentMut>()
            .map_err(|e| CliError::TomlParse {
                path: path.display().to_string(),
                reason: e.to_string(),
            })?;
        Ok(Self {
            path: path.to_path_buf(),
            doc,
        })
    }

    pub(crate) fn package_name(&self) -> Option<&str> {
        self.doc
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(|name| name.as_str())
    }

    /// The names the `[features]` table declares, none when it has no such table.
    pub(crate) fn feature_names(&self) -> Vec<String> {
        self.doc
            .get("features")
            .and_then(|features| features.as_table_like())
            .map(|features| features.iter().map(|(name, _)| name.to_owned()).collect())
            .unwrap_or_default()
    }

    /// Add `dep = { version, features }` unless a dependency of that name exists in any shape.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) fn ensure_dependency(&mut self, name: &str, version: &str, features: &[&str]) {
        let deps = self.dependencies_mut();
        if deps.contains_key(name) {
            debug!(dep = name, "dependency already present; no change");
            return;
        }
        let mut inline = toml_edit::InlineTable::new();
        inline.insert("version", Value::from(version));
        if !features.is_empty() {
            let mut arr = Array::new();
            for f in features {
                arr.push(*f);
            }
            inline.insert("features", Value::Array(arr));
        }
        deps.insert(name, value(Value::InlineTable(inline)));
        debug!(dep = name, "added dependency");
    }

    /// Ensure the crate's `fundamental` dependency enables `feature`, promoting a bare version string.
    ///
    /// # Errors
    /// [`CliError::TomlParse`] if the crate has no `fundamental` dependency.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) fn ensure_fundamental_feature(&mut self, feature: &str) -> Result<(), CliError> {
        let deps = self.dependencies_mut();
        let Some(item) = deps.get_mut("fundamental") else {
            return Err(CliError::TomlParse {
                path: self.path.display().to_string(),
                reason: "no `fundamental` dependency to attach a feature to".to_string(),
            });
        };

        if let Some(version) = item.as_str() {
            let mut inline = toml_edit::InlineTable::new();
            inline.insert("version", Value::from(version));
            *item = value(Value::InlineTable(inline));
        }

        let features = if let Some(inline) = item.as_inline_table_mut() {
            inline
                .entry("features")
                .or_insert_with(|| Value::Array(Array::new()))
                .as_array_mut()
        } else if let Some(table) = item.as_table_mut() {
            table
                .entry("features")
                .or_insert_with(|| Item::Value(Value::Array(Array::new())))
                .as_array_mut()
        } else {
            None
        };

        let Some(features) = features else {
            return Err(CliError::TomlParse {
                path: self.path.display().to_string(),
                reason: "`fundamental` dependency is not a table or string".to_string(),
            });
        };

        if features.iter().any(|v| v.as_str() == Some(feature)) {
            debug!(feature, "fundamental feature already enabled; no change");
            return Ok(());
        }
        features.push(feature);
        debug!(feature, "enabled fundamental feature");
        Ok(())
    }

    /// Declare an engine feature forwarding `fundamental/<engine>`, listed by `server`.
    ///
    /// `#[derive(Model)]` emits an engine's impls under `#[cfg(feature = "<engine>")]` of this crate.
    pub(crate) fn ensure_engine_capability(&mut self, engine: &str) {
        self.ensure_crate_feature(engine, &format!("fundamental/{engine}"));
        self.ensure_crate_feature("server", engine);
    }

    /// Ensure the `[features]` table has a feature `name` whose array contains `entry`.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) fn ensure_crate_feature(&mut self, name: &str, entry: &str) {
        let features = self
            .doc
            .entry("features")
            .or_insert_with(toml_edit::table)
            .as_table_mut()
            .expect("the features entry is always a table");
        let Some(array) = features
            .entry(name)
            .or_insert_with(|| Item::Value(Value::Array(Array::new())))
            .as_array_mut()
        else {
            debug!(feature = name, "crate feature is not an array; no change");
            return;
        };
        if array.iter().any(|v| v.as_str() == Some(entry)) {
            debug!(
                feature = name,
                entry, "crate feature already lists entry; no change"
            );
            return;
        }
        array.push(entry);
        debug!(feature = name, entry, "added entry to crate feature");
    }

    /// Write the document back to disk.
    ///
    /// # Errors
    /// [`CliError::Io`] if the file cannot be written.
    pub(crate) fn save(&self) -> Result<(), CliError> {
        fs::write(&self.path, self.doc.to_string()).map_err(|e| CliError::Io {
            path: self.path.display().to_string(),
            reason: e.to_string(),
        })
    }

    fn dependencies_mut(&mut self) -> &mut Table {
        self.doc
            .entry("dependencies")
            .or_insert_with(toml_edit::table)
            .as_table_mut()
            .expect("the dependencies entry is always a table")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn manifest_with(body: &str) -> (TempDir, CrateManifest) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("Cargo.toml");
        fs::write(&path, body).unwrap();
        let m = CrateManifest::load(&path).unwrap();
        (tmp, m)
    }

    #[test]
    fn adds_uuid_with_features() {
        let (_t, mut m) = manifest_with("[package]\nname = \"x\"\n\n[dependencies]\n");
        m.ensure_dependency("uuid", "1", &["v4"]);
        let out = m.doc.to_string();
        assert!(
            out.contains("uuid = { version = \"1\", features = [\"v4\"] }"),
            "{out}"
        );
    }

    #[test]
    fn an_existing_dependency_is_left_untouched() {
        let (_t, mut m) =
            manifest_with("[dependencies]\nuuid = { version = \"0.8\", features = [\"serde\"] }\n");
        m.ensure_dependency("uuid", "1", &["v4"]);
        let out = m.doc.to_string();
        assert!(out.contains("version = \"0.8\""), "{out}");
        assert!(!out.contains("\"v4\""), "{out}");
    }

    #[test]
    fn adds_a_feature_to_a_table_dependency() {
        let (_t, mut m) = manifest_with(
            "[dependencies]\nfundamental = { version = \"0.1.0\", features = [\"config\"] }\n",
        );
        m.ensure_fundamental_feature("sql").unwrap();
        let out = m.doc.to_string();
        assert!(out.contains("\"config\""), "{out}");
        assert!(out.contains("\"sql\""), "{out}");
    }

    #[test]
    fn promotes_a_bare_string_dependency_and_adds_features() {
        let (_t, mut m) = manifest_with("[dependencies]\nfundamental = \"0.1.0\"\n");
        m.ensure_fundamental_feature("cql").unwrap();
        let out = m.doc.to_string();
        assert!(out.contains("version = \"0.1.0\""), "{out}");
        assert!(out.contains("\"cql\""), "{out}");
    }

    #[test]
    fn enabling_an_existing_feature_is_a_noop() {
        let (_t, mut m) = manifest_with(
            "[dependencies]\nfundamental = { version = \"0.1.0\", features = [\"sql\"] }\n",
        );
        m.ensure_fundamental_feature("sql").unwrap();
        let out = m.doc.to_string();
        assert_eq!(out.matches("sql").count(), 1, "{out}");
    }

    #[test]
    fn a_crate_without_fundamental_is_an_error() {
        let (_t, mut m) = manifest_with("[dependencies]\nserde = \"1\"\n");
        let err = m
            .ensure_fundamental_feature("sql")
            .expect_err("no fundamental");
        assert!(matches!(err, CliError::TomlParse { .. }));
    }

    #[test]
    fn an_engine_capability_declares_its_own_feature_and_joins_server() {
        let (_t, mut m) = manifest_with("[features]\nserver = [\"fundamental/server\"]\n");
        m.ensure_engine_capability("sql");
        let out = m.doc.to_string();
        assert!(out.contains("sql = [\"fundamental/sql\"]"), "{out}");
        assert!(out.contains("\"fundamental/server\""), "{out}");
        assert!(out.contains("\"sql\""), "{out}");
    }

    #[test]
    fn an_engine_capability_creates_the_features_table_when_absent() {
        let (_t, mut m) = manifest_with("[package]\nname = \"x\"\n");
        m.ensure_engine_capability("cql");
        let out = m.doc.to_string();
        assert!(out.contains("[features]"), "{out}");
        assert!(out.contains("cql = [\"fundamental/cql\"]"), "{out}");
        assert!(out.contains("server = [\"cql\"]"), "{out}");
        assert!(!out.contains("target_arch"), "{out}");
    }

    #[test]
    fn declaring_the_same_engine_twice_is_a_noop() {
        let (_t, mut m) = manifest_with("[package]\nname = \"x\"\n");
        m.ensure_engine_capability("sql");
        m.ensure_engine_capability("sql");
        let out = m.doc.to_string();
        assert_eq!(out.matches("fundamental/sql").count(), 1, "{out}");
        assert_eq!(out.matches("server = ").count(), 1, "{out}");
    }

    #[test]
    fn adds_an_entry_to_an_existing_crate_feature() {
        let (_t, mut m) = manifest_with("[features]\nserver = [\"fundamental/server\"]\n");
        m.ensure_crate_feature("server", "fundamental/sql");
        let out = m.doc.to_string();
        assert!(out.contains("\"fundamental/server\""), "{out}");
        assert!(out.contains("\"fundamental/sql\""), "{out}");
    }

    #[test]
    fn creates_the_features_table_and_key_when_absent() {
        let (_t, mut m) = manifest_with("[package]\nname = \"x\"\n");
        m.ensure_crate_feature("server", "fundamental/cql");
        let out = m.doc.to_string();
        assert!(out.contains("[features]"), "{out}");
        assert!(out.contains("fundamental/cql"), "{out}");
    }

    #[test]
    fn adding_an_existing_crate_feature_entry_is_a_noop() {
        let (_t, mut m) = manifest_with("[features]\nserver = [\"fundamental/sql\"]\n");
        m.ensure_crate_feature("server", "fundamental/sql");
        let out = m.doc.to_string();
        assert_eq!(out.matches("fundamental/sql").count(), 1, "{out}");
    }
}
