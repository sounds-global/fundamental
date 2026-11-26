//! [`check`]: a crate's messages per language, and the messages its code never says.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr as _;
use std::sync::{Arc, Mutex};

use proc_macro2::{Delimiter, TokenStream, TokenTree};

use crate::{InputKind, Signature, signatures};

/// What a crate's messages lack or carry beyond what they should.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The source language, which every other is checked against.
    pub source: String,
    /// Each other language, in tag order.
    pub languages: Vec<Language>,
    /// The ids the source declares that no code of the crate says, in order.
    pub unused: Vec<String>,
    /// The error codes the code raises whose `error.<code>` message the source lacks, in order.
    pub undeclared: Vec<String>,
    /// The crate's wordings of a dependency's message that use an input it is not given, in order.
    pub mismatched: Vec<String>,
}

impl Report {
    /// Whether every message is said in every language, by the code, and declared.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.unused.is_empty()
            && self.undeclared.is_empty()
            && self.mismatched.is_empty()
            && self
                .languages
                .iter()
                .all(|language| language.missing.is_empty() && language.stale.is_empty())
    }
}

/// Each gap on its own line: `missing in nl: panel.save`.
impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for language in &self.languages {
            for id in &language.missing {
                writeln!(f, "missing in {}: {id}", language.tag)?;
            }
            for id in &language.stale {
                writeln!(f, "stale in {}: {id}", language.tag)?;
            }
        }
        for id in &self.unused {
            writeln!(f, "unused: {id}")?;
        }
        for id in &self.undeclared {
            writeln!(f, "undeclared in {}: {id}", self.source)?;
        }
        for id in &self.mismatched {
            writeln!(f, "mismatched in {}: {id}", self.source)?;
        }
        Ok(())
    }
}

/// One translation, checked against the source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Language {
    /// Its tag: `nl`.
    pub tag: String,
    /// The ids the source declares and it does not say, in order.
    pub missing: Vec<String>,
    /// The ids it says that the source no longer declares, in order.
    pub stale: Vec<String>,
}

/// The messages of the crate at `root` checked: each language under `locales/` against the
/// source language's, and the source's ids against what the crate's code says.
///
/// A message is said when the code names its function or a module above it, names its id in a
/// string, or raises the error whose code it carries; a dependency's message worded here is
/// said by that dependency.
///
/// # Errors
/// Each file that cannot be read, each source message that does not parse, and `cargo metadata`
/// failing for a crate that declares any message.
pub fn check(root: &Path) -> Result<Report, Vec<String>> {
    let source = mf2_build::Config::default().source_locale;
    let locales = root.join("locales");
    let own = signatures_in(&locales.join(&source))?;
    let declared: BTreeSet<String> = own.keys().cloned().collect();
    let mut languages = Vec::new();
    for folder in folders(&locales)? {
        let Some(tag) = folder.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if tag == source {
            continue;
        }
        let said = ids_in(&folder)?;
        languages.push(Language {
            tag: tag.to_owned(),
            missing: declared.difference(&said).cloned().collect(),
            stale: said.difference(&declared).cloned().collect(),
        });
    }
    languages.sort_by(|a, b| a.tag.cmp(&b.tag));
    let mut said = Said {
        name: library_name(root)?,
        ..Said::default()
    };
    for library in LIBRARY {
        said_in(&root.join(library), &mut said)?;
    }
    let raised = std::mem::take(&mut said.raised);
    for target in ["tests", "examples", "benches"] {
        said_in(&root.join(target), &mut said)?;
    }
    said.raised = raised;
    let unsaid: Vec<&String> = declared.iter().filter(|id| !said.says(id)).collect();
    let (unused, mismatched) = judge_by_dependencies(root, &source, &own, &unsaid)?;
    let undeclared = said.raised.difference(&declared).cloned().collect();
    Ok(Report {
        source,
        languages,
        unused,
        undeclared,
        mismatched,
    })
}

/// The crate's messages judged against its dependencies': unused when none declares them,
/// mismatched when worded with an input theirs is not given.
fn judge_by_dependencies(
    root: &Path,
    source: &str,
    own: &BTreeMap<String, Signature>,
    unsaid: &[&String],
) -> Result<(Vec<String>, Vec<String>), Vec<String>> {
    if own.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let theirs = dependencies_declare(root, source)?;
    let unused = unsaid
        .iter()
        .filter(|id| !theirs.contains_key(id.as_str()))
        .map(|id| (*id).clone())
        .collect();
    let mismatched = own
        .iter()
        .filter(|(id, ours)| {
            theirs
                .get(id.as_str())
                .is_some_and(|theirs| !fits(ours, theirs))
        })
        .map(|(id, _)| id.clone())
        .collect();
    Ok((unused, mismatched))
}

/// Whether wording `ours` uses only inputs `theirs` is given, a number only where it gives one.
fn fits(ours: &Signature, theirs: &Signature) -> bool {
    ours.inputs.iter().all(|input| {
        theirs.inputs.iter().any(|given| {
            given.name == input.name && (input.kind == InputKind::Text || input.kind == given.kind)
        })
    })
}

/// The name the crate at `root` is named by in code; none without a manifest.
fn library_name(root: &Path) -> Result<Option<String>, Vec<String>> {
    let path = root.join("Cargo.toml");
    if !path.is_file() {
        return Ok(None);
    }
    let manifest: toml::Table = read(&path)?
        .parse()
        .map_err(|error| vec![format!("{}: {error}", path.display())])?;
    let named = |table: &str| {
        manifest
            .get(table)
            .and_then(|table| table.get("name"))
            .and_then(toml::Value::as_str)
            .map(|name| name.replace('-', "_"))
    };
    Ok(named("lib").or_else(|| named("package")))
}

/// Every message the crates the one at `root` depends on declare, at any depth, by id.
fn dependencies_declare(
    root: &Path,
    source: &str,
) -> Result<BTreeMap<String, Signature>, Vec<String>> {
    let manifest = root.join("Cargo.toml");
    if !manifest.is_file() {
        return Ok(BTreeMap::new());
    }
    let metadata = metadata_of(&manifest)?;
    let mut found = BTreeMap::new();
    for folder in dependency_folders(metadata.as_ref(), &manifest) {
        let theirs = folder.join("locales").join(source);
        if theirs.is_dir() {
            for (id, signature) in signatures_in(&theirs)? {
                found.entry(id).or_insert(signature);
            }
        }
    }
    Ok(found)
}

/// What `cargo metadata` says of the workspace of `manifest`, read once per workspace per process.
fn metadata_of(manifest: &Path) -> Result<Arc<serde_json::Value>, Vec<String>> {
    static READ: Mutex<Vec<(BTreeSet<PathBuf>, Arc<serde_json::Value>)>> = Mutex::new(Vec::new());
    let canonical = manifest
        .canonicalize()
        .unwrap_or_else(|_| manifest.to_path_buf());
    let mut read = READ
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, metadata)) = read
        .iter()
        .find(|(members, _)| members.contains(&canonical))
    {
        return Ok(Arc::clone(metadata));
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = std::process::Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(manifest)
        .output()
        .map_err(|error| vec![format!("cargo metadata: {error}")])?;
    if !output.status.success() {
        return Err(vec![format!(
            "cargo metadata: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )]);
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| vec![format!("cargo metadata: {error}")])?;
    let members: Vec<&serde_json::Value> = metadata["workspace_members"]
        .as_array()
        .map_or_else(Vec::new, |members| members.iter().collect());
    let mut manifests: BTreeSet<PathBuf> = metadata["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|package| members.contains(&&package["id"]))
        .filter_map(|package| {
            Path::new(package["manifest_path"].as_str()?)
                .canonicalize()
                .ok()
        })
        .collect();
    manifests.insert(canonical);
    let metadata = Arc::new(metadata);
    read.push((manifests, Arc::clone(&metadata)));
    Ok(metadata)
}

/// The folder of every package the one with `manifest` depends on as a normal dependency, at any
/// depth, in the order they are reached.
fn dependency_folders(metadata: &serde_json::Value, manifest: &Path) -> Vec<PathBuf> {
    let packages = metadata["packages"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let manifest = manifest
        .canonicalize()
        .unwrap_or_else(|_| manifest.to_path_buf());
    let Some(start) = packages.iter().find_map(|package| {
        let path = Path::new(package["manifest_path"].as_str()?);
        (path.canonicalize().ok()? == manifest).then(|| package["id"].as_str())?
    }) else {
        return Vec::new();
    };
    let nodes: BTreeMap<&str, &serde_json::Value> = metadata["resolve"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|node| Some((node["id"].as_str()?, node)))
        .collect();
    let mut reached: Vec<&str> = vec![start];
    let mut seen: BTreeSet<&str> = BTreeSet::from([start]);
    let mut at = 0;
    while let Some(id) = reached.get(at).copied() {
        at += 1;
        let Some(node) = nodes.get(id) else {
            continue;
        };
        for dependency in node["deps"].as_array().into_iter().flatten() {
            let normal = dependency["dep_kinds"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|kind| kind["kind"].is_null());
            if let Some(pkg) = dependency["pkg"].as_str()
                && normal
                && seen.insert(pkg)
            {
                reached.push(pkg);
            }
        }
    }
    let folders: BTreeMap<&str, &str> = packages
        .iter()
        .filter_map(|package| Some((package["id"].as_str()?, package["manifest_path"].as_str()?)))
        .collect();
    reached
        .into_iter()
        .skip(1)
        .filter_map(|id| Some(Path::new(folders.get(id)?).parent()?.to_path_buf()))
        .collect()
}

/// The folders a crate's library is built from: `src/`, and an app's `pages/` and `seeds/`.
const LIBRARY: [&str; 3] = ["src", "pages", "seeds"];

/// The ids of every message in a language's folder.
fn ids_in(folder: &Path) -> Result<BTreeSet<String>, Vec<String>> {
    Ok(signatures_in(folder)?.into_keys().collect())
}

/// Every message in a language's folder, by id with its signature.
fn signatures_in(folder: &Path) -> Result<BTreeMap<String, Signature>, Vec<String>> {
    let mut ids = BTreeMap::new();
    for path in files(folder, "mf2")? {
        let text = read(&path)?;
        let found = signatures(&text).map_err(|problems| {
            problems
                .into_iter()
                .map(|problem| format!("{}: {problem}", path.display()))
                .collect::<Vec<_>>()
        })?;
        ids.extend(
            found
                .into_iter()
                .map(|signature| (signature.id.clone(), signature)),
        );
    }
    Ok(ids)
}

/// What a crate's code says: the message paths it names, its strings and its error codes.
#[derive(Default)]
struct Said {
    /// The name the crate is named by in code, which may qualify its own `m`.
    name: Option<String>,
    paths: BTreeSet<String>,
    strings: BTreeSet<String>,
    /// The ids of the errors the code raises; only the library's and binaries' count.
    raised: BTreeSet<String>,
}

impl Said {
    /// Whether message `id` is said: named itself, or by a module above it.
    fn says(&self, id: &str) -> bool {
        self.strings.contains(id)
            || self.raised.contains(id)
            || self.paths.iter().any(|path| {
                id == path
                    || id
                        .strip_prefix(path.as_str())
                        .is_some_and(|rest| rest.starts_with('.'))
            })
    }

    fn read(&mut self, tokens: TokenStream) {
        let tokens: Vec<TokenTree> = tokens.into_iter().collect();
        let mut at = 0;
        while let Some(token) = tokens.get(at) {
            let after = &tokens[at + 1..];
            if test_only(token, after) {
                let end = at + 2 + item_length(&after[1..]);
                self.test_item(&tokens[at + 2..end]);
                at = end;
                continue;
            }
            let theirs = matches!(token, TokenTree::Ident(ident) if ident == "m")
                && names_another_crate(&tokens[..at], self.name.as_deref());
            if !theirs {
                self.token(token, after);
            }
            at += 1;
        }
    }

    /// A `#[cfg(test)]` item: its messages count, its error codes do not.
    fn test_item(&mut self, item: &[TokenTree]) {
        let raised = std::mem::take(&mut self.raised);
        for (at, token) in item.iter().enumerate() {
            self.token(token, &item[at + 1..]);
        }
        self.raised = raised;
    }

    /// One token, read with the ones after it.
    fn token(&mut self, token: &TokenTree, after: &[TokenTree]) {
        match token {
            TokenTree::Ident(ident) if ident == "m" => {
                self.paths.extend(message_path(after));
            }
            TokenTree::Ident(ident) if ident == "error" => {
                if let Some(TokenTree::Group(group)) = after.first()
                    && group.delimiter() == Delimiter::Parenthesis
                {
                    self.error_codes(group.stream());
                }
            }
            TokenTree::Literal(literal) => self.strings.extend(string(literal)),
            TokenTree::Group(group) => self.read(group.stream()),
            _ => {}
        }
    }

    /// The `code = "…"` of an `#[error(…)]`, said as its message's id.
    fn error_codes(&mut self, tokens: TokenStream) {
        let tokens: Vec<TokenTree> = tokens.into_iter().collect();
        for window in tokens.windows(3) {
            if let [
                TokenTree::Ident(name),
                TokenTree::Punct(equals),
                TokenTree::Literal(value),
            ] = window
                && name == "code"
                && equals.as_char() == '='
                && let Some(code) = string(value)
            {
                self.raised
                    .insert(format!("error.{}", code.to_ascii_lowercase()));
            }
        }
    }
}

/// Whether the tokens before an `m` qualify it with another crate's name (`ui::m`): `crate`,
/// `self`, `super` and the crate's own `name` qualify its own.
fn names_another_crate(before: &[TokenTree], name: Option<&str>) -> bool {
    let [
        ..,
        TokenTree::Ident(qualifier),
        TokenTree::Punct(first),
        TokenTree::Punct(second),
    ] = before
    else {
        return false;
    };
    first.as_char() == ':'
        && second.as_char() == ':'
        && !["crate", "self", "super"].contains(&qualifier.to_string().as_str())
        && name.is_none_or(|name| *qualifier != name)
}

/// Whether `token` and the one after it are `#[cfg(test)]`.
fn test_only(token: &TokenTree, after: &[TokenTree]) -> bool {
    matches!(token, TokenTree::Punct(hash) if hash.as_char() == '#')
        && matches!(after.first(), Some(TokenTree::Group(group))
            if group.delimiter() == Delimiter::Bracket
                && group.stream().to_string().replace(' ', "") == "cfg(test)")
}

/// How many tokens the item at the start of `tokens` takes: up to its body or its `;`.
fn item_length(tokens: &[TokenTree]) -> usize {
    tokens
        .iter()
        .position(|token| match token {
            TokenTree::Group(group) => group.delimiter() == Delimiter::Brace,
            TokenTree::Punct(punct) => punct.as_char() == ';',
            _ => false,
        })
        .map_or(tokens.len(), |end| end + 1)
}

/// The message path after an `m`: `:: panel :: save` is `panel.save`.
fn message_path(tokens: &[TokenTree]) -> Option<String> {
    let mut segments = Vec::new();
    let mut rest = tokens;
    while let [
        TokenTree::Punct(first),
        TokenTree::Punct(second),
        TokenTree::Ident(name),
        tail @ ..,
    ] = rest
    {
        if first.as_char() != ':' || second.as_char() != ':' {
            break;
        }
        segments.push(name.to_string().trim_start_matches("r#").to_owned());
        rest = tail;
    }
    (!segments.is_empty()).then(|| segments.join("."))
}

/// A plain string literal's text.
fn string(literal: &proc_macro2::Literal) -> Option<String> {
    let text = literal.to_string();
    let inner = text.strip_prefix('"')?.strip_suffix('"')?;
    (!inner.contains('\\')).then(|| inner.to_owned())
}

/// What the code under `src` says, added to `said`.
fn said_in(src: &Path, said: &mut Said) -> Result<(), Vec<String>> {
    if !src.is_dir() {
        return Ok(());
    }
    for path in files(src, "rs")? {
        let text = read(&path)?;
        let tokens = TokenStream::from_str(&text)
            .map_err(|error| vec![format!("{}: {error}", path.display())])?;
        said.read(tokens);
    }
    Ok(())
}

/// Every file with `extension` under `folder`, at any depth, in order.
fn files(folder: &Path, extension: &str) -> Result<Vec<PathBuf>, Vec<String>> {
    let mut found = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(here) = pending.pop() {
        for entry in std::fs::read_dir(&here)
            .map_err(|error| vec![format!("{}: {error}", here.display())])?
        {
            let path = entry
                .map_err(|error| vec![format!("{}: {error}", here.display())])?
                .path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|found| found == extension) {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The language folders under `locales`.
fn folders(locales: &Path) -> Result<Vec<PathBuf>, Vec<String>> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(locales)
        .map_err(|error| vec![format!("{}: {error}", locales.display())])?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    found.sort();
    Ok(found)
}

fn read(path: &Path) -> Result<String, Vec<String>> {
    std::fs::read_to_string(path).map_err(|error| vec![format!("{}: {error}", path.display())])
}
