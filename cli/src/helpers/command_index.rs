//! [`CommandIndex`]: every address in a project, read without building anything.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fundamental::CommandAddress;
use tracing::{debug, instrument};

use crate::CliError;
use crate::helpers::{CommandEntry, CommandSource, Manifest, Project};

/// Every command a project can run, keyed by address, read from sources so listing builds nothing.
///
/// `fun run --list --verify` checks it against each binary's own `--list`.
#[derive(Debug, Default)]
pub(crate) struct CommandIndex {
    entries: BTreeMap<String, CommandEntry>,
}

impl CommandIndex {
    /// Builds the index from the manifest's `[commands]` and every app's `#[command]`s.
    ///
    /// # Errors
    /// [`CliError::ManifestCommandShape`] for an unreadable entry, [`CliError::DuplicateCommandAddress`]
    /// when two sources claim one address.
    #[instrument(skip_all, fields(root = %project.root().display()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) fn build(project: &Project, manifest: &Manifest) -> Result<Self, CliError> {
        let mut index = Self::default();

        for script in manifest.commands()? {
            let address = CommandAddress::new(&script.namespace, &script.name)
                .expect("the manifest reader validated both halves");
            index.insert(CommandEntry {
                address,
                description: script.description,
                source: CommandSource::Script {
                    declared_in: fundamental_manifest::Manifest::FILE.to_owned(),
                    run: script.run,
                },
            })?;
        }

        for app in project.apps_with_commands() {
            for entry in scan_app(project.root(), &app)? {
                index.insert(entry)?;
            }
        }

        debug!(addresses = index.entries.len(), "built the command index");
        Ok(index)
    }

    fn insert(&mut self, entry: CommandEntry) -> Result<(), CliError> {
        let address = entry.address_string();
        if let Some(existing) = self.entries.get(&address) {
            return Err(CliError::DuplicateCommandAddress {
                address,
                first: existing.source.to_string(),
                second: entry.source.to_string(),
            });
        }
        self.entries.insert(address, entry);
        Ok(())
    }

    pub(crate) fn get(&self, address: &str) -> Option<&CommandEntry> {
        self.entries.get(address)
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = &CommandEntry> {
        self.entries.values()
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn by_namespace(&self) -> BTreeMap<&str, Vec<&CommandEntry>> {
        let mut grouped: BTreeMap<&str, Vec<&CommandEntry>> = BTreeMap::new();
        for entry in self.entries.values() {
            grouped
                .entry(entry.address.namespace())
                .or_default()
                .push(entry);
        }
        grouped
    }

    /// The addresses declared by `app`'s `commands` binary, sorted, for the drift gate.
    pub(crate) fn addresses_for_app(&self, app: &str) -> Vec<String> {
        self.entries
            .values()
            .filter(|e| e.source.app() == Some(app))
            .map(CommandEntry::address_string)
            .collect()
    }

    /// The address closest to `requested`, within a third of its length.
    pub(crate) fn nearest(&self, requested: &str) -> Option<&str> {
        let budget = (requested.chars().count() / 3).max(1);
        self.entries
            .keys()
            .map(|address| (edit_distance(requested, address), address))
            .filter(|(distance, _)| *distance <= budget)
            .min_by_key(|(distance, address)| (*distance, address.len()))
            .map(|(_, address)| address.as_str())
    }
}

fn scan_app(root: &Path, app: &str) -> Result<Vec<CommandEntry>, CliError> {
    let bin = root.join("apps").join(app).join("src").join("bin");
    let mut sources = Vec::new();
    let single = bin.join("commands.rs");
    if single.is_file() {
        sources.push(single);
    }
    let directory = bin.join("commands");
    if directory.is_dir() {
        let mut listing = std::fs::read_dir(&directory)
            .map_err(|e| CliError::Io {
                path: directory.display().to_string(),
                reason: e.to_string(),
            })?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "rs"))
            .collect::<Vec<_>>();
        listing.sort();
        sources.extend(listing);
    }

    let mut entries = Vec::new();
    for path in sources {
        entries.extend(scan_file(root, app, &path)?);
    }
    Ok(entries)
}

#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "readability baseline"
)]
fn scan_file(root: &Path, app: &str, path: &PathBuf) -> Result<Vec<CommandEntry>, CliError> {
    let text = std::fs::read_to_string(path).map_err(|e| CliError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    // An unparseable file is cargo's to report; listing must not fail on unrelated mid-edit sources.
    let Ok(file) = syn::parse_file(&text) else {
        debug!(path = %path.display(), "skipped a file that does not parse");
        return Ok(Vec::new());
    };
    let declared_in = path
        .strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string();

    let mut entries = Vec::new();
    for item in file.items {
        let syn::Item::Fn(function) = item else {
            continue;
        };
        let Some(attribute) = function.attrs.iter().find(|a| a.path().is_ident("command")) else {
            continue;
        };
        let mut namespace: Option<String> = None;
        let mut name: Option<String> = None;
        let mut description: Option<String> = None;
        // Skipped rather than refused: the compiler reports it better, and the drift gate proves agreement.
        let parsed = attribute.parse_nested_meta(|meta| {
            let target = if meta.path.is_ident("namespace") {
                &mut namespace
            } else if meta.path.is_ident("name") {
                &mut name
            } else if meta.path.is_ident("description") {
                &mut description
            } else {
                return Ok(());
            };
            let literal: syn::LitStr = meta.value()?.parse()?;
            *target = Some(literal.value());
            Ok(())
        });
        if parsed.is_err() {
            debug!(path = %declared_in, "skipped an unparsable #[command] attribute");
            continue;
        }

        let Some(namespace) = namespace else {
            debug!(path = %declared_in, "skipped a #[command] with no namespace");
            continue;
        };
        // The same fallback the macro applies.
        let name = name.unwrap_or_else(|| function.sig.ident.to_string().replace('_', "-"));
        let Ok(address) = CommandAddress::new(&namespace, &name) else {
            debug!(path = %declared_in, "skipped a #[command] whose address is not legal");
            continue;
        };
        entries.push(CommandEntry {
            address,
            description,
            source: CommandSource::Rust {
                app: app.to_owned(),
                declared_in: declared_in.clone(),
            },
        });
    }
    Ok(entries)
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right_chars: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right_chars.len()).collect();
    let mut current = vec![0; right_chars.len() + 1];
    for (i, l) in left.chars().enumerate() {
        current[0] = i + 1;
        for (j, r) in right_chars.iter().enumerate() {
            let substitution = previous[j] + usize::from(l != *r);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right_chars.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn project(manifest_text: &str, app: Option<(&str, &str)>) -> (TempDir, Project, Manifest) {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("fun.toml"), manifest_text).unwrap();
        if let Some((name, source)) = app {
            let dir = tmp
                .path()
                .join("apps")
                .join(name)
                .join("src")
                .join("bin")
                .join("commands");
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("main.rs"), "fn main() {}").unwrap();
            fs::write(dir.join("thing.rs"), source).unwrap();
        }
        let project = Project::discover_from(tmp.path()).unwrap();
        let manifest = Manifest::load(&project.manifest_path()).unwrap();
        (tmp, project, manifest)
    }

    #[test]
    fn an_empty_project_has_no_addresses() {
        let (_t, p, m) = project("[project]\n", None);
        let index = CommandIndex::build(&p, &m).expect("builds");
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn a_namespace_sub_table_becomes_addresses() {
        let (_t, p, m) = project(
            "[project]\n[commands.gate]\ndocs = \"scripts/docs-check.sh\"\n\
             layering = { run = \"scripts/layering-check.sh\", description = \"edges\" }\n",
            None,
        );
        let index = CommandIndex::build(&p, &m).expect("builds");
        assert_eq!(index.len(), 2);
        assert_eq!(
            index.get("gate:layering").unwrap().description.as_deref(),
            Some("edges")
        );
        assert_eq!(index.get("gate:docs").unwrap().source.app(), None);
    }

    #[test]
    fn an_entry_with_no_namespace_is_refused_rather_than_skipped() {
        let (_t, p, m) = project("[project]\n[commands]\nlint = \"cargo clippy\"\n", None);
        let err = CommandIndex::build(&p, &m).expect_err("a command belongs to a namespace");
        assert!(
            matches!(err, CliError::ManifestCommandShape { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn a_table_without_a_run_key_is_refused_rather_than_skipped() {
        let (_t, p, m) = project(
            "[project]\n[commands.gate]\ndocs = { description = \"no run line\" }\n",
            None,
        );
        let err = CommandIndex::build(&p, &m).expect_err("no run key");
        match err {
            CliError::ManifestCommandShape { name, reason, .. } => {
                assert_eq!(name, "docs");
                assert!(reason.contains("run"), "{reason}");
            }
            other => panic!("expected ManifestCommandShape, got {other:?}"),
        }
    }

    #[test]
    fn rust_commands_are_read_from_the_sources() {
        let source = r#"
            #[command(
                namespace = "mint",
                name = "operator-token",
                description = "Mint a scoped bearer"
            )]
            pub(crate) async fn mint_operator_token(_ctx: &Cli, args: CommandArgs) -> Result<(), CommandError> { Ok(()) }

            /// Not a command.
            fn helper() {}
        "#;
        let (_t, p, m) = project("[project]\n", Some(("layer0", source)));
        let index = CommandIndex::build(&p, &m).expect("builds");
        assert_eq!(index.len(), 1);
        let entry = index.get("mint:operator-token").expect("scanned");
        assert_eq!(entry.description.as_deref(), Some("Mint a scoped bearer"));
        assert_eq!(entry.source.app(), Some("layer0"));
        assert_eq!(
            index.addresses_for_app("layer0"),
            vec!["mint:operator-token"]
        );
    }

    #[test]
    fn a_defaulted_name_is_the_kebab_case_identifier() {
        let source = r#"
            #[command(namespace = "mint")]
            pub async fn operator_token(_ctx: &Cli) -> Result<(), CommandError> { Ok(()) }
        "#;
        let (_t, p, m) = project("[project]\n", Some(("layer0", source)));
        let index = CommandIndex::build(&p, &m).expect("builds");
        assert!(index.get("mint:operator-token").is_some(), "{index:?}");
    }

    #[test]
    fn a_script_and_a_command_cannot_share_an_address() {
        // A script shadowing a Rust command of its own name, whose line invokes that name, recurses forever.
        let source = r#"
            #[command(namespace = "bootstrap", name = "seed")]
            pub async fn seed(_ctx: &Cli) -> Result<(), CommandError> { Ok(()) }
        "#;
        let (_t, p, m) = project(
            "[project]\n[commands.bootstrap]\nseed = \"fun run bootstrap:seed\"\n",
            Some(("layer0", source)),
        );
        let err = CommandIndex::build(&p, &m).expect_err("one address, one owner");
        match err {
            CliError::DuplicateCommandAddress { address, .. } => {
                assert_eq!(address, "bootstrap:seed");
            }
            other => panic!("expected DuplicateCommandAddress, got {other:?}"),
        }
    }

    #[test]
    fn one_name_in_two_namespaces_is_not_a_collision() {
        let (_t, p, m) = project(
            "[project]\n[commands.business]\nsession-keys = \"a\"\n\
             [commands.holder]\nsession-keys = \"b\"\n",
            None,
        );
        let index = CommandIndex::build(&p, &m).expect("distinct addresses");
        assert_eq!(index.len(), 2);
    }

    #[test]
    fn a_former_flat_name_suggests_its_address() {
        let (_t, p, m) = project(
            "[project]\n[commands.bootstrap]\nkeys = \"a\"\nseed = \"b\"\n",
            None,
        );
        let index = CommandIndex::build(&p, &m).expect("builds");
        assert_eq!(index.nearest("bootstrap-keys"), Some("bootstrap:keys"));
        assert_eq!(index.nearest("bootstrap-seed"), Some("bootstrap:seed"));
    }

    #[test]
    fn something_unrelated_suggests_nothing() {
        let (_t, p, m) = project("[project]\n[commands.bootstrap]\nkeys = \"a\"\n", None);
        assert_eq!(index_of(&p, &m).nearest("deploy:aircraft-carrier"), None);
    }

    fn index_of(p: &Project, m: &Manifest) -> CommandIndex {
        CommandIndex::build(p, m).expect("builds")
    }

    #[test]
    fn grouping_is_by_namespace_and_sorted() {
        let (_t, p, m) = project(
            "[project]\n[commands.gate]\ndocs = \"a\"\nalerts = \"b\"\n[commands.deploy]\nci = \"c\"\n",
            None,
        );
        let index = CommandIndex::build(&p, &m).expect("builds");
        let grouped = index.by_namespace();
        assert_eq!(
            grouped.keys().copied().collect::<Vec<_>>(),
            vec!["deploy", "gate"]
        );
        assert_eq!(
            grouped["gate"]
                .iter()
                .map(|e| e.address.name())
                .collect::<Vec<_>>(),
            vec!["alerts", "docs"]
        );
    }

    #[test]
    fn edit_distance_counts_single_edits() {
        assert_eq!(edit_distance("", ""), 0);
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert_eq!(edit_distance("bootstrap-keys", "bootstrap:keys"), 1);
        assert_eq!(edit_distance("", "abc"), 3);
    }
}
