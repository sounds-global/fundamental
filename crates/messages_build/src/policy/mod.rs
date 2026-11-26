//! [`Policy`]: what a missing, stale or unused message is, per environment.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::Report;

/// How a gap is treated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Not reported.
    Allow,
    /// Reported; the build and the check go on.
    Warn,
    /// Reported; the build and the check fail.
    Error,
}

impl Level {
    fn parse(word: &str) -> Option<Self> {
        match word {
            "allow" => Some(Self::Allow),
            "warn" => Some(Self::Warn),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Allow => "allow",
            Self::Warn => "warn",
            Self::Error => "error",
        })
    }
}

/// What a language shows for a message it does not say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gap {
    /// The text of the first language its chain names that says it, marked as borrowed.
    Borrowed,
    /// The message's id, so the gap is seen on the page.
    Id,
    /// Nothing.
    Nothing,
}

impl Gap {
    fn parse(word: &str) -> Option<Self> {
        match word {
            "borrowed" => Some(Self::Borrowed),
            "id" => Some(Self::Id),
            "nothing" => Some(Self::Nothing),
            _ => None,
        }
    }
}

/// Where the messages are built: a developer's machine or CI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Environment {
    /// A developer's machine.
    Local,
    /// Continuous integration, which every CI says by setting `CI`.
    Ci,
}

impl Environment {
    /// CI when `CI` is set to anything but `false`, `0` or nothing.
    #[must_use]
    pub fn from_ci(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            None | Some("" | "false" | "0") => Self::Local,
            Some(_) => Self::Ci,
        }
    }

    /// The environment this process runs in.
    #[must_use]
    pub fn detect() -> Self {
        Self::from_ci(std::env::var("CI").ok().as_deref())
    }

    const fn key(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Ci => "ci",
        }
    }
}

/// How each kind of gap is judged in one environment, and how a missing message is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    /// A message the source declares that a language does not say.
    pub missing: Level,
    /// A message a language says that the source no longer declares.
    pub stale: Level,
    /// A message the source declares that no code says.
    pub unused: Level,
    /// What a language shows for a message it does not say.
    pub gap: Gap,
    /// Per language, the languages it borrows a message from, in order, before the source.
    pub fallback: BTreeMap<String, Vec<String>>,
}

impl Policy {
    /// The defaults: a gap warns locally and fails in CI; a stale message fails everywhere.
    #[must_use]
    pub const fn default_for(environment: Environment) -> Self {
        match environment {
            Environment::Local => Self {
                missing: Level::Warn,
                stale: Level::Error,
                unused: Level::Warn,
                gap: Gap::Borrowed,
                fallback: BTreeMap::new(),
            },
            Environment::Ci => Self {
                missing: Level::Error,
                stale: Level::Error,
                unused: Level::Error,
                gap: Gap::Borrowed,
                fallback: BTreeMap::new(),
            },
        }
    }

    /// The policy for the crate at `root` in `environment`: the defaults, with what the nearest
    /// `fun.toml` sets under `[messages.<local|ci>]` and `[messages.fallback]`.
    ///
    /// # Errors
    /// The manifest cannot be read or parsed, or names a level, gap or chain it does not know.
    pub fn read(root: &Path, environment: Environment) -> Result<Self, String> {
        let mut policy = Self::default_for(environment);
        let Some(manifest) = Self::manifest(root) else {
            return Ok(policy);
        };
        let table = Self::load(&manifest)?;
        let table = table.as_table();
        let Some(messages) = table.get("messages") else {
            return Ok(policy);
        };
        if let Some(chains) = messages.get("fallback") {
            policy.fallback = chains_of(chains).map_err(|problem| {
                format!("{}: [messages.fallback] {problem}", manifest.display())
            })?;
        }
        let Some(levels) = messages.get(environment.key()) else {
            return Ok(policy);
        };
        if let Some(value) = levels.get("gap") {
            policy.gap = value.as_str().and_then(Gap::parse).ok_or_else(|| {
                format!(
                    "{}: [messages.{}] gap = {value} is not \"borrowed\", \"id\" or \"nothing\"",
                    manifest.display(),
                    environment.key()
                )
            })?;
        }
        for (name, level) in [
            ("missing", &mut policy.missing),
            ("stale", &mut policy.stale),
            ("unused", &mut policy.unused),
        ] {
            let Some(value) = levels.get(name) else {
                continue;
            };
            *level = value.as_str().and_then(Level::parse).ok_or_else(|| {
                format!(
                    "{}: [messages.{}] {name} = {value} is not \"allow\", \"warn\" or \"error\"",
                    manifest.display(),
                    environment.key()
                )
            })?;
        }
        Ok(policy)
    }

    /// The nearest `fun.toml` at or above `root`.
    #[must_use]
    pub fn manifest(root: &Path) -> Option<PathBuf> {
        root.ancestors()
            .map(|folder| folder.join("fun.toml"))
            .find(|path| path.is_file())
    }

    /// Every file the policy for the crate at `root` is read from, for a build to rerun on.
    #[must_use]
    pub fn manifests(root: &Path) -> Vec<PathBuf> {
        let Some(manifest) = Self::manifest(root) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = Self::load(&manifest)
            .map(|loaded| {
                loaded
                    .files()
                    .iter()
                    .map(|file| loaded.root().join(file))
                    .collect()
            })
            .unwrap_or_default();
        if !files.contains(&manifest) {
            files.insert(0, manifest);
        }
        files
    }

    /// The project's settings whose `fun.toml` is `manifest`.
    fn load(manifest: &Path) -> Result<fundamental_manifest::Manifest, String> {
        let project = manifest.parent().unwrap_or_else(|| Path::new("."));
        fundamental_manifest::Manifest::load(project)
            .map_err(|error| format!("{}: {error}", manifest.display()))
    }
}

/// Each language's chain under `[messages.fallback]`: a list of languages per language.
fn chains_of(value: &toml::Value) -> Result<BTreeMap<String, Vec<String>>, String> {
    let table = value
        .as_table()
        .ok_or_else(|| "is not a table of chains".to_owned())?;
    table
        .iter()
        .map(|(language, chain)| {
            let languages = chain
                .as_array()
                .and_then(|items| {
                    items
                        .iter()
                        .map(|item| item.as_str().map(str::to_owned))
                        .collect::<Option<Vec<_>>>()
                })
                .ok_or_else(|| format!("{language} = {chain} is not a list of languages"))?;
            Ok((language.clone(), languages))
        })
        .collect()
}

/// What kind of gap a finding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The source declares it and the language does not say it.
    Missing,
    /// The language says it and the source no longer declares it.
    Stale,
    /// The source declares it and no code says it.
    Unused,
    /// The code raises it and the source does not declare it, judged as missing.
    Undeclared,
    /// It words a crate's message with an input that message is not given, judged as stale.
    Mismatched,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Missing => "missing",
            Self::Stale => "stale",
            Self::Unused => "unused",
            Self::Undeclared => "undeclared",
            Self::Mismatched => "mismatched",
        })
    }
}

/// One gap, judged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// What the policy makes it.
    pub level: Level,
    /// What kind of gap it is.
    pub kind: Kind,
    /// The language it is in; none for an unused message.
    pub language: Option<String>,
    /// The message's id.
    pub id: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.language {
            Some(language) => write!(f, "{}: {} {} in {language}", self.level, self.kind, self.id),
            None => write!(f, "{}: {} {}", self.level, self.kind, self.id),
        }
    }
}

impl Report {
    /// Every gap judged by `policy`, leaving out those it allows.
    #[must_use]
    pub fn findings(&self, policy: &Policy) -> Vec<Finding> {
        let mut found = Vec::new();
        for language in &self.languages {
            for (kind, level, ids) in [
                (Kind::Missing, policy.missing, &language.missing),
                (Kind::Stale, policy.stale, &language.stale),
            ] {
                found.extend(ids.iter().map(|id| Finding {
                    level,
                    kind,
                    language: Some(language.tag.clone()),
                    id: id.clone(),
                }));
            }
        }
        found.extend(self.unused.iter().map(|id| Finding {
            level: policy.unused,
            kind: Kind::Unused,
            language: None,
            id: id.clone(),
        }));
        found.extend(self.undeclared.iter().map(|id| Finding {
            level: policy.missing,
            kind: Kind::Undeclared,
            language: Some(self.source.clone()),
            id: id.clone(),
        }));
        found.extend(self.mismatched.iter().map(|id| Finding {
            level: policy.stale,
            kind: Kind::Mismatched,
            language: Some(self.source.clone()),
            id: id.clone(),
        }));
        found.retain(|finding| finding.level != Level::Allow);
        found
    }
}
