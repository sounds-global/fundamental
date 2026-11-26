//! [`RawPending`], one `[[<table>.pending]]` entry as it is written.

use serde::Deserialize;

/// One `[[<table>.pending]]` entry as written, before [`PendingLedger::read`] checks it.
///
/// [`PendingLedger::read`]: super::PendingLedger::read
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPending {
    /// The file, relative to the project.
    pub file: String,
    /// The code of the rule it freezes.
    pub rule: String,
    /// The crate a frozen finding reaches, for a rule whose name key is `crate`.
    #[serde(rename = "crate", default)]
    pub krate: Option<String>,
    /// The item a frozen finding sits in, for a rule whose name key is `item`.
    #[serde(default)]
    pub item: Option<String>,
    /// How many findings the entry freezes.
    pub count: u32,
    /// The work that pays them.
    pub until: String,
}

impl RawPending {
    /// The name the entry gives under `key`, if it gives one that is not blank.
    pub(crate) fn name_under(&self, key: &str) -> Option<&str> {
        let name = match key {
            "crate" => self.krate.as_deref(),
            "item" => self.item.as_deref(),
            _ => None,
        };
        name.filter(|name| !name.trim().is_empty())
    }

    /// Every name key the entry gives a value under.
    pub(crate) fn named_keys(&self) -> Vec<&'static str> {
        ["crate", "item"]
            .into_iter()
            .filter(|key| self.name_under(key).is_some())
            .collect()
    }
}
