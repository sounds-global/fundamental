//! [`PendingLedger`], the exact list of findings a project has frozen for one check.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;
use std::path::{Component, Path, PathBuf};

use fundamental_core::Variant;

use super::{Held, PendingEntry, PendingMismatch, PendingRule, RawPending, Unheld};

/// The findings a project has frozen for one check, each until named work pays it; any
/// difference from an exact entry, either way, is [`Unheld`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingLedger<R> {
    entries: Vec<PendingEntry<R>>,
    at: String,
}

impl<R> Default for PendingLedger<R> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            at: String::new(),
        }
    }
}

impl<R: PendingRule> PendingLedger<R> {
    /// The ledger the `raw` entries written at `at` (`path:line`) declare; every wrong entry is
    /// pushed to `problems` and left out.
    #[must_use]
    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub fn read(raw: Vec<RawPending>, at: String, problems: &mut Vec<String>) -> Self {
        let mut entries = Vec::with_capacity(raw.len());
        let mut seen = BTreeSet::new();
        for entry in raw {
            let mut refuse = |reason: String| problems.push(reason);
            let Some(rule) = R::from_value(&entry.rule) else {
                refuse(format!(
                    "the pending entry for '{}' names `{}`; only {} can be pending",
                    entry.file,
                    entry.rule,
                    listed(R::OPTIONS.iter().map(Variant::value), "and")
                ));
                continue;
            };
            let Some(count) = NonZeroU32::new(entry.count) else {
                refuse(format!(
                    "the pending entry for '{}' ({rule}) freezes no finding; remove it",
                    entry.file
                ));
                continue;
            };
            if entry.until.trim().is_empty() {
                refuse(format!(
                    "the pending entry for '{}' ({rule}) says nothing in `until`; name the work that moves it",
                    entry.file
                ));
            }
            let file = PathBuf::from(&entry.file);
            if !is_inside_the_project(&file) {
                refuse(format!(
                    "the pending entry for '{}' names a path outside the project; write it relative to the project",
                    entry.file
                ));
                continue;
            }
            let name = match rule.name_key() {
                Some(key) => {
                    let Some(name) = entry.name_under(key) else {
                        refuse(format!(
                            "the pending entry for '{}' ({rule}) names no `{key}`; a frozen finding of {rule} names it, so another cannot take its place",
                            entry.file
                        ));
                        continue;
                    };
                    if let Some(other) = entry.named_keys().into_iter().find(|named| *named != key)
                    {
                        refuse(unexpected_name(&entry, rule, other));
                        continue;
                    }
                    Some(name.to_owned())
                }
                None => {
                    if let Some(other) = entry.named_keys().first() {
                        refuse(unexpected_name(&entry, rule, other));
                        continue;
                    }
                    None
                }
            };
            if !seen.insert((file.clone(), rule, name.clone())) {
                refuse(format!(
                    "'{}' is pending for {rule}{} twice; one entry holds the whole count",
                    entry.file,
                    name.as_ref()
                        .map_or_else(String::new, |name| format!(" in `{name}`"))
                ));
                continue;
            }
            entries.push(PendingEntry {
                file,
                rule,
                name,
                count,
                until: entry.until,
            });
        }
        Self { entries, at }
    }

    /// The ledger's entries, in the order they are written.
    #[must_use]
    pub fn entries(&self) -> &[PendingEntry<R>] {
        &self.entries
    }

    /// Where the ledger is written, as `path:line`.
    #[must_use]
    pub fn at(&self) -> &str {
        &self.at
    }

    /// How many findings of `rule` in `file` under `name` the ledger freezes: what a check of
    /// that one file may find and stay green.
    #[must_use]
    pub fn allowance(&self, file: &Path, rule: R, name: Option<&str>) -> u32 {
        self.entries
            .iter()
            .find(|entry| entry.file == file && entry.rule == rule && entry.name.as_deref() == name)
            .map_or(0, |entry| entry.count.get())
    }

    /// Holds `found`, a check's findings keyed by file, rule and name, against the ledger;
    /// entries `in_scope` declines are skipped unless their file is gone.
    pub fn hold<F>(
        &self,
        root: &Path,
        mut found: BTreeMap<(PathBuf, R, Option<String>), Vec<F>>,
        in_scope: impl Fn(&PendingEntry<R>) -> bool,
    ) -> Held<F> {
        let mut unheld = Vec::new();
        let mut frozen = 0;
        for entry in &self.entries {
            let mismatch = |found: String, advice: &str| {
                Unheld::Mismatch(PendingMismatch {
                    at: self.at.clone(),
                    file: entry.file.display().to_string(),
                    rule: entry.rule.value(),
                    name: entry.name.clone(),
                    frozen: entry.count.get(),
                    found,
                    advice: advice.to_owned(),
                })
            };
            if !root.join(&entry.file).is_file() {
                unheld.push(mismatch(
                    "there is no such file".to_owned(),
                    "Remove the entry, or correct its path.",
                ));
                continue;
            }
            if !in_scope(entry) {
                continue;
            }
            let held = found.remove(&(entry.file.clone(), entry.rule, entry.name.clone()));
            let count = held.as_ref().map_or(0, Vec::len);
            let owed = usize::try_from(entry.count.get()).unwrap_or(usize::MAX);
            match count.cmp(&owed) {
                Ordering::Equal => frozen += count,
                Ordering::Less if count == 0 => unheld.push(mismatch(
                    "found none".to_owned(),
                    "The debt is paid: remove the entry.",
                )),
                Ordering::Less => unheld.push(mismatch(
                    format!("found {count}"),
                    &format!("Lower the count to {count}."),
                )),
                Ordering::Greater => {
                    unheld.push(mismatch(
                        format!("found {count}"),
                        "A frozen file takes no new finding: fix the new one, listed below.",
                    ));
                    unheld.extend(held.into_iter().flatten().map(Unheld::Finding));
                }
            }
        }
        unheld.extend(found.into_values().flatten().map(Unheld::Finding));
        Held { frozen, unheld }
    }
}

/// The refusal for an entry that gives a name its rule does not take.
fn unexpected_name<R: PendingRule>(entry: &RawPending, rule: R, key: &str) -> String {
    let takers = R::OPTIONS
        .iter()
        .filter(|option| option.name_key() == Some(key))
        .map(Variant::value);
    let takers: Vec<&str> = takers.collect();
    if takers.is_empty() {
        format!(
            "the pending entry for '{}' ({rule}) names a `{key}`; no rule here takes one",
            entry.file
        )
    } else {
        format!(
            "the pending entry for '{}' ({rule}) names the {key} `{}`; only a {} entry names one",
            entry.file,
            entry.name_under(key).unwrap_or_default(),
            listed(takers.into_iter(), "or")
        )
    }
}

/// `a`, `a and b`, `a, b and c`.
fn listed<'a>(items: impl Iterator<Item = &'a str>, last: &str) -> String {
    let items: Vec<&str> = items.collect();
    match items.split_last() {
        None => String::new(),
        Some((only, [])) => (*only).to_owned(),
        Some((tail, head)) => format!("{} {last} {tail}", head.join(", ")),
    }
}

fn is_inside_the_project(path: &Path) -> bool {
    path.components().all(|c| matches!(c, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use std::fmt;
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Rule {
        Plain,
        Named,
    }

    impl Variant for Rule {
        const KEY: &'static str = "rule";
        const OPTIONS: &'static [Self] = &[Self::Plain, Self::Named];

        fn value(&self) -> &'static str {
            match self {
                Self::Plain => "T_001",
                Self::Named => "T_002",
            }
        }

        fn from_value(value: &str) -> Option<Self> {
            Self::OPTIONS
                .iter()
                .copied()
                .find(|rule| rule.value() == value)
        }
    }

    impl fmt::Display for Rule {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.value())
        }
    }

    impl PendingRule for Rule {
        fn name_key(self) -> Option<&'static str> {
            (self == Self::Named).then_some("item")
        }
    }

    fn raw(file: &str, rule: &str, item: Option<&str>, count: u32) -> RawPending {
        RawPending {
            file: file.to_owned(),
            rule: rule.to_owned(),
            krate: None,
            item: item.map(str::to_owned),
            count,
            until: "issue 1".to_owned(),
        }
    }

    fn ledger(entries: Vec<RawPending>) -> (PendingLedger<Rule>, Vec<String>) {
        let mut problems = Vec::new();
        let ledger = PendingLedger::read(entries, "ledger.toml:1".to_owned(), &mut problems);
        (ledger, problems)
    }

    /// A project holding `a.rs` and `b.rs`.
    fn project() -> TempDir {
        let root = TempDir::new().unwrap_or_else(|e| panic!("a temp dir: {e}"));
        for file in ["a.rs", "b.rs"] {
            fs::write(root.path().join(file), "").unwrap_or_else(|e| panic!("writing {file}: {e}"));
        }
        root
    }

    type Found = BTreeMap<(PathBuf, Rule, Option<String>), Vec<&'static str>>;

    fn found(findings: &[(&str, Rule, Option<&str>, &'static str)]) -> Found {
        let mut map = Found::new();
        for (file, rule, name, finding) in findings {
            map.entry((PathBuf::from(file), *rule, name.map(str::to_owned)))
                .or_default()
                .push(*finding);
        }
        map
    }

    fn mismatches(held: &Held<&'static str>) -> Vec<String> {
        held.unheld
            .iter()
            .filter_map(|unheld| match unheld {
                Unheld::Mismatch(m) => Some(format!("{} {}: {}", m.file, m.found, m.advice)),
                Unheld::Finding(_) => None,
            })
            .collect()
    }

    fn findings(held: &Held<&'static str>) -> Vec<&'static str> {
        held.unheld
            .iter()
            .filter_map(|unheld| match unheld {
                Unheld::Finding(f) => Some(*f),
                Unheld::Mismatch(_) => None,
            })
            .collect()
    }

    #[test]
    fn a_frozen_file_holding_its_exact_count_is_green() {
        let root = project();
        let (ledger, problems) = ledger(vec![raw("a.rs", "T_001", None, 2)]);
        assert!(problems.is_empty(), "{problems:?}");
        let held = ledger.hold(
            root.path(),
            found(&[
                ("a.rs", Rule::Plain, None, "x"),
                ("a.rs", Rule::Plain, None, "y"),
            ]),
            |_| true,
        );
        assert!(held.is_clean(), "{held:?}");
        assert_eq!(held.frozen, 2);
    }

    #[test]
    fn one_finding_more_than_frozen_is_red_and_lists_the_findings() {
        let root = project();
        let (ledger, _) = ledger(vec![raw("a.rs", "T_001", None, 1)]);
        let held = ledger.hold(
            root.path(),
            found(&[
                ("a.rs", Rule::Plain, None, "x"),
                ("a.rs", Rule::Plain, None, "y"),
            ]),
            |_| true,
        );
        assert_eq!(
            mismatches(&held),
            ["a.rs found 2: A frozen file takes no new finding: fix the new one, listed below."]
        );
        assert_eq!(findings(&held), ["x", "y"]);
        assert_eq!(held.frozen, 0);
    }

    #[test]
    fn one_fewer_asks_to_lower_the_count_and_none_to_remove_the_entry() {
        let root = project();
        let (ledger, _) = ledger(vec![
            raw("a.rs", "T_001", None, 3),
            raw("b.rs", "T_001", None, 1),
        ]);
        let held = ledger.hold(
            root.path(),
            found(&[("a.rs", Rule::Plain, None, "x")]),
            |_| true,
        );
        assert_eq!(
            mismatches(&held),
            [
                "a.rs found 1: Lower the count to 1.",
                "b.rs found none: The debt is paid: remove the entry."
            ]
        );
    }

    #[test]
    fn an_entry_for_a_missing_file_is_red_even_out_of_scope() {
        let root = project();
        let (ledger, _) = ledger(vec![raw("gone.rs", "T_001", None, 1)]);
        let held = ledger.hold(root.path(), Found::new(), |_| false);
        assert_eq!(
            mismatches(&held),
            ["gone.rs there is no such file: Remove the entry, or correct its path."]
        );
    }

    #[test]
    fn swapping_debt_between_items_is_red() {
        let root = project();
        let (ledger, _) = ledger(vec![raw("a.rs", "T_002", Some("Paid"), 1)]);
        let held = ledger.hold(
            root.path(),
            found(&[("a.rs", Rule::Named, Some("Fresh"), "fresh codec")]),
            |_| true,
        );
        assert_eq!(
            mismatches(&held),
            ["a.rs found none: The debt is paid: remove the entry."]
        );
        assert_eq!(findings(&held), ["fresh codec"]);
    }

    #[test]
    fn an_unfrozen_finding_is_unheld() {
        let root = project();
        let (ledger, _) = ledger(Vec::new());
        let held = ledger.hold(
            root.path(),
            found(&[("b.rs", Rule::Plain, None, "new")]),
            |_| true,
        );
        assert_eq!(findings(&held), ["new"]);
    }

    #[test]
    fn every_way_an_entry_is_wrong_is_named() {
        let mut blank_until = raw("a.rs", "T_001", None, 1);
        blank_until.until = " ".to_owned();
        let mut crate_named = raw("b.rs", "T_002", Some("Item"), 1);
        crate_named.krate = Some("store".to_owned());
        let (ledger, problems) = ledger(vec![
            raw("a.rs", "T_009", None, 1),
            raw("a.rs", "T_001", None, 0),
            blank_until,
            raw("../a.rs", "T_001", None, 1),
            raw("a.rs", "T_002", None, 1),
            raw("a.rs", "T_001", Some("Item"), 1),
            crate_named,
            raw("b.rs", "T_001", None, 1),
            raw("b.rs", "T_001", None, 2),
        ]);
        let joined = problems.join("\n");
        for expected in [
            "names `T_009`; only T_001 and T_002 can be pending",
            "freezes no finding",
            "says nothing in `until`",
            "outside the project",
            "names no `item`",
            "names the item `Item`; only a T_002 entry names one",
            "names a `crate`; no rule here takes one",
            "pending for T_001 twice",
        ] {
            assert!(joined.contains(expected), "{expected} in\n{joined}");
        }
        assert_eq!(problems.len(), 8, "{joined}");
        assert_eq!(
            ledger.entries().len(),
            2,
            "the blank `until` is named and kept"
        );
    }

    #[test]
    fn allowance_is_the_frozen_count_or_zero() {
        let (ledger, _) = ledger(vec![
            raw("a.rs", "T_001", None, 3),
            raw("a.rs", "T_002", Some("Kind"), 2),
        ]);
        assert_eq!(ledger.allowance(Path::new("a.rs"), Rule::Plain, None), 3);
        assert_eq!(
            ledger.allowance(Path::new("a.rs"), Rule::Named, Some("Kind")),
            2
        );
        assert_eq!(
            ledger.allowance(Path::new("a.rs"), Rule::Named, Some("Other")),
            0
        );
        assert_eq!(ledger.allowance(Path::new("b.rs"), Rule::Plain, None), 0);
    }
}
