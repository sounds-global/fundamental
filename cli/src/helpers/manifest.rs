//! The project's [`Manifest`] as the CLI reads and edits it, keeping every comment.

use std::path::{Path, PathBuf};

use fundamental_manifest::{KeyPath, Manifest as ProjectManifest, ManifestEditor};
use toml::Value as Plain;
use toml_edit::{Array, Item, Table, Value, value};
use tracing::{debug, instrument};

use fundamental::CommandAddress;

use crate::CliError;
use crate::Surface;
use crate::engine::{DbConfigValue, Engine};
use crate::helpers::ManifestCommand;

/// A project's manifest, readable and editable; [`save`](Self::save) writes the changed files.
#[derive(Debug)]
pub(crate) struct Manifest {
    path: PathBuf,
    editor: ManifestEditor,
    view: ProjectManifest,
}

impl Manifest {
    /// Load the manifest whose root `fun.toml` is at `path`.
    ///
    /// # Errors
    /// [`CliError::Manifest`] when the manifest cannot be read or is refused.
    #[instrument(skip_all, fields(path = %path.display()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) fn load(path: &Path) -> Result<Self, CliError> {
        let root = path.parent().unwrap_or_else(|| Path::new(""));
        let editor = ManifestEditor::open(root)?;
        let view = editor.manifest()?;
        Ok(Self {
            path: path.to_path_buf(),
            editor,
            view,
        })
    }

    pub(crate) fn project_name(&self) -> Option<String> {
        self.view
            .get(&key(&["project", "name"]))?
            .as_str()
            .map(str::to_owned)
    }

    pub(crate) fn has_app(&self, app: &str) -> bool {
        self.apps().is_some_and(|apps| apps.contains_key(app))
    }

    /// The app `[apps.<app>]` is an instance of (`instance_of`), when it is one.
    pub(crate) fn instance_base(&self, app: &str) -> Option<String> {
        self.app(app)?
            .get("instance_of")?
            .as_str()
            .map(str::to_owned)
    }

    /// Whether `[apps.<app>]` declares `surface`, or the base it is an instance of does.
    pub(crate) fn has_surface(&self, app: &str, surface: Surface) -> bool {
        self.owners(app)
            .any(|table| table.contains_key(surface.as_str()))
    }

    /// The `[apps.<app>.web].css_command` run before the bundle is compiled, if any.
    pub(crate) fn web_css_command(&self, app: &str) -> Option<String> {
        self.owners(app).find_map(|table| {
            table
                .get("web")?
                .get("css_command")?
                .as_str()
                .map(str::to_owned)
        })
    }

    /// The cargo features `fun dev` builds with, from `[dev] features`, so dev and the bar share one set.
    pub(crate) fn dev_features(&self) -> Option<Vec<String>> {
        let features = self.view.get(&key(&["dev", "features"]))?;
        Some(
            features
                .as_array()?
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect(),
        )
    }

    /// Every command in the `[commands.<namespace>]` tables, sorted by address.
    ///
    /// # Errors
    /// [`CliError::ManifestCommandShape`] for an entry outside a namespace, a non-kebab-case
    /// namespace, or an entry that is neither a string nor a table with `run`.
    pub(crate) fn commands(&self) -> Result<Vec<ManifestCommand>, CliError> {
        let Some(table) = self
            .view
            .as_table()
            .get("commands")
            .and_then(Plain::as_table)
        else {
            return Ok(Vec::new());
        };
        let mut commands = Vec::new();
        for (namespace, item) in table {
            let Some(entries) = item.as_table() else {
                return Err(self.command_shape(
                    namespace,
                    namespace,
                    "is a bare entry directly under `[commands]`; every command belongs to a \
                     namespace, so move it into a `[commands.<namespace>]` table",
                ));
            };
            if let Err(objection) = CommandAddress::validate_segment(namespace) {
                return Err(self.command_shape(
                    namespace,
                    namespace,
                    &format!("is not a legal namespace: it {objection}"),
                ));
            }
            for (name, entry) in entries {
                commands.push(self.manifest_command(namespace, name, entry)?);
            }
        }
        commands.sort_by_key(ManifestCommand::address);
        Ok(commands)
    }

    /// Add or replace a `[commands.<namespace>].<name>` entry with the shell line `run`.
    ///
    /// # Errors
    /// [`CliError::Manifest`] when the edit leaves an invalid manifest.
    pub(crate) fn set_command(
        &mut self,
        namespace: &str,
        name: &str,
        run: &str,
    ) -> Result<(), CliError> {
        self.editor
            .table_mut(&key(&["commands", namespace]))?
            .insert(name, value(run));
        debug!(command = format!("{namespace}:{name}"), "set command");
        self.refresh()
    }

    /// Accepts `name = "cmd"` and `name = { run = "cmd", description = "…" }`.
    ///
    /// # Errors
    /// [`CliError::ManifestCommandShape`] for any other shape, or a name not in kebab-case.
    fn manifest_command(
        &self,
        namespace: &str,
        name: &str,
        item: &Plain,
    ) -> Result<ManifestCommand, CliError> {
        if let Err(objection) = CommandAddress::validate_segment(name) {
            return Err(self.command_shape(
                namespace,
                name,
                &format!("is not a legal command name: it {objection}"),
            ));
        }
        let (run, description) = if let Some(run) = item.as_str() {
            (run.to_owned(), None)
        } else {
            let table = item.as_table().ok_or_else(|| {
                self.command_shape(namespace, name, "is neither a shell line nor a table")
            })?;
            let run = table
                .get("run")
                .and_then(Plain::as_str)
                .ok_or_else(|| self.command_shape(namespace, name, "has no `run` string"))?
                .to_owned();
            let description = table
                .get("description")
                .and_then(Plain::as_str)
                .map(str::to_owned);
            (run, description)
        };
        if run.trim().is_empty() {
            return Err(self.command_shape(namespace, name, "has an empty `run` line"));
        }
        Ok(ManifestCommand {
            namespace: namespace.to_owned(),
            name: name.to_owned(),
            run,
            description,
        })
    }

    fn command_shape(&self, namespace: &str, name: &str, reason: &str) -> CliError {
        let file = self
            .path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(ProjectManifest::FILE);
        CliError::ManifestCommandShape {
            path: file.display().to_string(),
            namespace: namespace.to_owned(),
            name: name.to_owned(),
            reason: reason.to_owned(),
        }
    }

    /// The distinct database engines wired across every app, YSQL first.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub(crate) fn database_engines(&self) -> Vec<Engine> {
        let mut engines: Vec<Engine> = Vec::new();
        let Some(apps) = self.apps() else {
            return engines;
        };
        for app in apps.values() {
            let Some(database) = app.get("database").and_then(Plain::as_table) else {
                continue;
            };
            for key in database.keys() {
                let engine = Engine::parse(key);
                if !engines.contains(&engine) {
                    engines.push(engine);
                }
            }
        }
        engines.sort_by_key(|engine| match engine {
            Engine::SqlDb => 0,
            Engine::CqlDb => 1,
            Engine::Other(_) => 2,
        });
        engines
    }

    pub(crate) fn app_names(&self) -> Vec<String> {
        self.apps()
            .map(|apps| apps.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Resolve the named app, or the project's sole app when none is named.
    ///
    /// # Errors
    /// [`CliError::AppNotFound`], [`CliError::NoApps`] or [`CliError::AmbiguousApps`].
    pub(crate) fn resolve_app(&self, requested: Option<String>) -> Result<String, CliError> {
        if let Some(app) = requested {
            if self.has_app(&app) {
                return Ok(app);
            }
            return Err(CliError::AppNotFound { app });
        }
        let mut apps = self.app_names();
        match apps.len() {
            0 => Err(CliError::NoApps),
            1 => Ok(apps.remove(0)),
            _ => Err(CliError::AmbiguousApps {
                apps: apps.join(", "),
            }),
        }
    }

    /// Add an `[apps.<app>]` table with `surfaces`; a socket gets port `9000 + index`.
    ///
    /// # Errors
    /// [`CliError::Manifest`] when the edit leaves an invalid manifest.
    pub(crate) fn add_app(
        &mut self,
        app: &str,
        surfaces: &[Surface],
        socket_port: u16,
    ) -> Result<(), CliError> {
        let table = self.editor.table_mut(&key(&["apps", app]))?;
        for surface in surfaces {
            insert_surface(table, *surface, socket_port);
        }
        debug!(app, "added app table");
        self.refresh()
    }

    /// Add an `[apps.<app>.database.<engine>]` block with the defaults `fun new` writes.
    ///
    /// # Errors
    /// [`CliError::Manifest`] when the edit leaves an invalid manifest.
    pub(crate) fn add_database(
        &mut self,
        app: &str,
        db: &str,
        engine: &Engine,
    ) -> Result<(), CliError> {
        let table = self
            .editor
            .table_mut(&key(&["apps", app, "database", engine.config_key()]))?;
        for (field, config) in engine.scaffold_config(db) {
            table.insert(field, db_config_item(&config));
        }
        debug!(app, engine = engine.config_key(), "added database block");
        self.refresh()
    }

    /// Add a single `surface` to an existing `[apps.<app>]` table.
    ///
    /// # Errors
    /// [`CliError::AppNotFound`] or [`CliError::Manifest`].
    pub(crate) fn add_surface(
        &mut self,
        app: &str,
        surface: Surface,
        socket_port: u16,
    ) -> Result<(), CliError> {
        if !self.has_app(app) {
            return Err(CliError::AppNotFound {
                app: app.to_string(),
            });
        }
        insert_surface(
            self.editor.table_mut(&key(&["apps", app]))?,
            surface,
            socket_port,
        );
        debug!(app, surface = surface.as_str(), "added surface");
        self.refresh()
    }

    /// Set the `css_command` on an app's existing `web` surface table; a no-op without one.
    ///
    /// # Errors
    /// [`CliError::Manifest`] when the edit leaves an invalid manifest.
    pub(crate) fn set_web_css_command(&mut self, app: &str, command: &str) -> Result<(), CliError> {
        if !self.has_surface(app, Surface::Web) {
            return Ok(());
        }
        if let Some(web) = self
            .editor
            .table_mut(&key(&["apps", app]))?
            .get_mut("web")
            .and_then(Item::as_inline_table_mut)
        {
            web.insert("css_command", Value::from(command));
            debug!(app, "set web css_command");
        }
        self.refresh()
    }

    /// Write every changed file back to disk.
    ///
    /// # Errors
    /// [`CliError::Manifest`] if a file cannot be written, changed since it was read, or is invalid.
    pub(crate) fn save(&mut self) -> Result<(), CliError> {
        self.editor.save()?;
        Ok(())
    }

    fn refresh(&mut self) -> Result<(), CliError> {
        self.view = self.editor.manifest()?;
        Ok(())
    }

    fn apps(&self) -> Option<&toml::Table> {
        self.view.as_table().get("apps").and_then(Plain::as_table)
    }

    fn app(&self, app: &str) -> Option<&toml::Table> {
        self.apps()?.get(app).and_then(Plain::as_table)
    }

    /// The app's table, then its base's: what an instance leaves out is its base's.
    fn owners<'a>(&'a self, app: &str) -> impl Iterator<Item = &'a toml::Table> {
        let base = self.instance_base(app);
        [self.app(app), base.and_then(|base| self.app(&base))]
            .into_iter()
            .flatten()
    }
}

fn key(segments: &[&str]) -> KeyPath {
    KeyPath::from_segments(segments.iter().copied())
}

fn db_config_item(config: &DbConfigValue) -> Item {
    match config {
        DbConfigValue::Str(s) => value(s.as_str()),
        DbConfigValue::List(items) => {
            let mut array = Array::new();
            for item in items {
                array.push(item.as_str());
            }
            value(array)
        }
    }
}

fn insert_surface(app_table: &mut Table, surface: Surface, port: u16) {
    match surface {
        Surface::Worker | Surface::Native => {
            app_table.insert(
                surface.as_str(),
                value(Value::InlineTable(Default::default())),
            );
        }
        Surface::Socket | Surface::Api | Surface::Web | Surface::Sync => {
            let mut inline = toml_edit::InlineTable::new();
            inline.insert("port", Value::from(i64::from(port)));
            app_table.insert(surface.as_str(), value(Value::InlineTable(inline)));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use tempfile::TempDir;

    fn written(dir: &TempDir, mut m: Manifest) -> String {
        m.save().unwrap();
        fs::read_to_string(dir.path().join("fun.toml")).unwrap()
    }

    fn manifest_with(body: &str) -> (TempDir, Manifest) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("fun.toml");
        fs::write(&path, body).unwrap();
        let m = Manifest::load(&path).unwrap();
        (tmp, m)
    }

    #[test]
    fn add_app_appends_a_table_with_surfaces() {
        let (_t, mut m) = manifest_with("[project]\nname = \"acme\"\n");
        assert!(!m.has_app("billing"));
        m.add_app("billing", &[Surface::Worker, Surface::Socket], 9001)
            .unwrap();
        assert!(m.has_app("billing"));
        let out = written(&_t, m);
        assert!(out.contains("[apps.billing]"), "{out}");
        assert!(out.contains("worker = {}"), "{out}");
        assert!(out.contains("socket = { port = 9001 }"), "{out}");
    }

    #[test]
    fn add_database_writes_a_sql_block_matching_the_template() {
        let (_t, mut m) = manifest_with("[project]\nname = \"acme\"\n");
        m.add_app("billing", &[Surface::Api], 9001).unwrap();
        m.add_database("billing", "billing", &Engine::SqlDb)
            .unwrap();
        let out = written(&_t, m);
        assert!(out.contains("[apps.billing.database.sql]"), "{out}");
        assert!(
            out.contains("url = \"postgresql://yugabyte@localhost:5433/billing?sslmode=disable\""),
            "{out}"
        );
        assert!(!out.contains("[apps.billing.database]\n"), "{out}");
    }

    #[test]
    fn add_database_writes_cql_hosts_and_keyspace() {
        let (_t, mut m) = manifest_with("[project]\nname = \"acme\"\n");
        m.add_app("events", &[Surface::Sync], 9002).unwrap();
        m.add_database("events", "events", &Engine::CqlDb).unwrap();
        let out = written(&_t, m);
        assert!(out.contains("[apps.events.database.cql]"), "{out}");
        assert!(out.contains("hosts = [\"127.0.0.1:9042\"]"), "{out}");
        assert!(out.contains("keyspace = \"events\""), "{out}");
    }

    #[test]
    fn add_database_is_read_back_by_database_engines() {
        let (_t, mut m) = manifest_with("[project]\nname = \"acme\"\n");
        m.add_app("shop", &[Surface::Api], 9003).unwrap();
        m.add_database("shop", "shop", &Engine::SqlDb).unwrap();
        m.add_database("shop", "shop", &Engine::CqlDb).unwrap();
        assert_eq!(m.database_engines(), vec![Engine::SqlDb, Engine::CqlDb]);
    }

    #[test]
    fn editing_preserves_comments() {
        let (_t, mut m) =
            manifest_with("# my project\n[project]\nname = \"acme\"\n\n[apps.main]\nworker = {}\n");
        m.add_app("extra", &[Surface::Worker], 9001).unwrap();
        let out = written(&_t, m);
        assert!(out.contains("# my project"), "comment dropped: {out}");
        assert!(out.contains("[apps.main]"), "existing app dropped: {out}");
    }

    #[test]
    fn add_surface_requires_an_existing_app() {
        let (_t, mut m) = manifest_with("[project]\nname = \"acme\"\n");
        let err = m
            .add_surface("ghost", Surface::Worker, 9000)
            .expect_err("no such app");
        assert!(matches!(err, CliError::AppNotFound { .. }));
    }

    #[test]
    fn add_surface_extends_an_existing_app() {
        let (_t, mut m) = manifest_with("[apps.main]\nworker = {}\n");
        assert!(!m.has_surface("main", Surface::Socket));
        m.add_surface("main", Surface::Socket, 9000).unwrap();
        assert!(m.has_surface("main", Surface::Socket));
        assert!(written(&_t, m).contains("socket = { port = 9000 }"));
    }

    #[test]
    fn resolve_app_picks_the_sole_app_or_the_named_one() {
        let (_t, m) = manifest_with("[apps.main]\nworker = {}\n");
        assert_eq!(m.app_names(), vec!["main".to_string()]);
        assert_eq!(m.resolve_app(None).unwrap(), "main");
        assert_eq!(m.resolve_app(Some("main".to_string())).unwrap(), "main");
    }

    #[test]
    fn resolve_app_rejects_an_unknown_named_app() {
        use fundamental::core::Error as _;
        let (_t, m) = manifest_with("[apps.main]\nworker = {}\n");
        let err = m.resolve_app(Some("ghost".to_string())).unwrap_err();
        assert!(matches!(err, CliError::AppNotFound { .. }));
        assert_eq!(err.error_code(), "CLI_009");
    }

    #[test]
    fn resolve_app_errors_when_the_project_has_no_apps() {
        use fundamental::core::Error as _;
        let (_t, m) = manifest_with("[project]\nname = \"acme\"\n");
        assert!(m.app_names().is_empty());
        let err = m.resolve_app(None).unwrap_err();
        assert!(matches!(err, CliError::NoApps));
        assert_eq!(err.error_code(), "CLI_024");
    }

    #[test]
    fn resolve_app_errors_when_ambiguous_and_lists_the_apps() {
        use fundamental::core::Error as _;
        let (_t, m) =
            manifest_with("[apps.main]\nworker = {}\n\n[apps.billing]\napi = { port = 16001 }\n");
        assert_eq!(m.app_names().len(), 2);
        let err = m.resolve_app(None).unwrap_err();
        match &err {
            CliError::AmbiguousApps { apps } => {
                assert!(apps.contains("main") && apps.contains("billing"), "{apps}");
            }
            other => panic!("expected AmbiguousApps, got {other:?}"),
        }
        assert_eq!(err.error_code(), "CLI_025");
    }

    #[test]
    fn database_engines_is_empty_without_a_database() {
        let (_t, m) = manifest_with("[apps.main]\nworker = {}\n");
        assert!(m.database_engines().is_empty());
    }

    #[test]
    fn database_engines_reads_each_apps_backend() {
        let (_t, m) = manifest_with(
            "[apps.main]\nsync = { port = 8787 }\n\n\
             [apps.main.database.cql]\nhosts = [\"127.0.0.1:9042\"]\nkeyspace = \"main\"\n",
        );
        assert_eq!(m.database_engines(), vec![Engine::CqlDb]);
    }

    #[test]
    fn database_engines_dedupes_and_orders_sql_before_cql() {
        let (_t, m) = manifest_with(
            "[apps.main]\nsync = { port = 8787 }\n\n\
             [apps.main.database.cql]\nhosts = [\"127.0.0.1:9042\"]\nkeyspace = \"main\"\n\n\
             [apps.billing]\napi = { port = 16001 }\n\n\
             [apps.billing.database.sql]\nurl = \"postgresql://localhost/billing\"\n",
        );
        assert_eq!(m.database_engines(), vec![Engine::SqlDb, Engine::CqlDb]);
    }

    #[test]
    fn commands_read_the_bare_string_form() {
        let (_t, m) = manifest_with("[commands.gate]\nlint = \"cargo clippy --all-targets\"\n");
        let commands = m.commands().expect("legal");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].address(), "gate:lint");
        assert_eq!(commands[0].run, "cargo clippy --all-targets");
        assert_eq!(commands[0].description, None);
    }

    #[test]
    fn commands_read_the_table_form_with_description() {
        let (_t, m) = manifest_with(
            "[commands.deploy]\nci = { run = \"./ship.sh\", description = \"Ship it\" }\n",
        );
        let commands = m.commands().expect("legal");
        assert_eq!(commands[0].address(), "deploy:ci");
        assert_eq!(commands[0].run, "./ship.sh");
        assert_eq!(commands[0].description.as_deref(), Some("Ship it"));
    }

    #[test]
    fn a_table_entry_without_a_run_key_is_refused_not_dropped() {
        let (_t, m) = manifest_with("[commands.gate]\nbroken = { description = \"no run key\" }\n");
        let err = m.commands().expect_err("no run key");
        match err {
            CliError::ManifestCommandShape { name, reason, .. } => {
                assert_eq!(name, "broken");
                assert!(reason.contains("run"), "{reason}");
            }
            other => panic!("expected ManifestCommandShape, got {other:?}"),
        }
    }

    #[test]
    fn an_empty_run_line_is_refused() {
        let (_t, m) = manifest_with("[commands.gate]\nblank = \"   \"\n");
        assert!(matches!(
            m.commands(),
            Err(CliError::ManifestCommandShape { .. })
        ));
    }

    #[test]
    fn a_command_outside_a_namespace_is_refused() {
        let (_t, m) = manifest_with("[commands]\nlint = \"cargo clippy\"\n");
        let err = m.commands().expect_err("a command belongs to a namespace");
        match err {
            CliError::ManifestCommandShape { reason, .. } => {
                assert!(reason.contains("namespace"), "{reason}");
            }
            other => panic!("expected ManifestCommandShape, got {other:?}"),
        }
    }

    #[test]
    fn a_namespace_that_is_not_kebab_case_is_refused() {
        let (_t, m) = manifest_with("[commands.Gate]\ndocs = \"a\"\n");
        assert!(matches!(
            m.commands(),
            Err(CliError::ManifestCommandShape { .. })
        ));
    }

    #[test]
    fn commands_are_empty_without_a_commands_table() {
        let (_t, m) = manifest_with("[project]\nname = \"acme\"\n");
        assert!(m.commands().expect("legal").is_empty());
    }

    #[test]
    fn commands_are_listed_sorted_by_address() {
        let (_t, m) = manifest_with(
            "[commands.zeta]\nlast = \"z\"\n\
             [commands.alpha]\nfirst = { run = \"a\", description = \"first\" }\n\
             beta = \"b\"\n",
        );
        let addresses: Vec<String> = m
            .commands()
            .expect("legal")
            .iter()
            .map(ManifestCommand::address)
            .collect();
        assert_eq!(
            addresses,
            vec![
                "alpha:beta".to_string(),
                "alpha:first".to_string(),
                "zeta:last".to_string()
            ]
        );
    }

    #[test]
    fn set_command_adds_a_namespace_table_preserving_comments() {
        let (_t, mut m) = manifest_with("# top\n[project]\nname = \"acme\"\n");
        m.set_command("gate", "lint", "cargo clippy --all-targets")
            .unwrap();
        let commands = m.commands().expect("legal");
        assert_eq!(commands[0].address(), "gate:lint");
        assert_eq!(commands[0].run, "cargo clippy --all-targets");
        let out = written(&_t, m);
        assert!(out.contains("# top"), "comment dropped: {out}");
        assert!(out.contains("[commands.gate]"), "{out}");
        assert!(!out.contains("\n[commands]\n"), "{out}");
    }

    #[test]
    fn invalid_toml_is_a_parse_error() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("fun.toml");
        fs::write(&path, "this is = = not toml").unwrap();
        let err = Manifest::load(&path).expect_err("bad toml");
        assert!(matches!(err, CliError::Manifest(_)), "{err:?}");
    }

    #[test]
    fn an_edit_is_written_to_fun_toml_and_never_to_the_machines_file() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("fun.toml"),
            "[project]\nname = \"acme\"\n\n[apps.main]\nworker = {}\n",
        )
        .unwrap();
        fs::write(
            tmp.path().join("fun.local.toml"),
            "[apps.main.worker]\nconcurrency = 2\n",
        )
        .unwrap();
        let mut m = Manifest::load(&tmp.path().join("fun.toml")).unwrap();
        assert!(m.has_app("main"));
        m.add_app("billing", &[Surface::Api], 16001).unwrap();
        m.set_command("gate", "lint", "cargo clippy").unwrap();
        m.save().unwrap();
        let settings = fs::read_to_string(tmp.path().join("fun.toml")).unwrap();
        assert!(
            settings.contains("[apps.billing]") && settings.contains("api = { port = 16001 }"),
            "{settings}"
        );
        assert!(
            settings.contains("[commands.gate]\nlint = \"cargo clippy\""),
            "{settings}"
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("fun.local.toml")).unwrap(),
            "[apps.main.worker]\nconcurrency = 2\n"
        );
    }
}
