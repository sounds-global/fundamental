//! Optional, focused dependency rules for a project.

use std::collections::BTreeMap;

use fundamental_manifest::pending::{PendingLedger, RawPending};
use fundamental_manifest::{KeyPath, Manifest};
use serde::Deserialize;
use tracing::debug;

use super::architecture_finding::ArchitectureFinding;
use super::file_rule::FileRule;

/// Explicit dependency edges a project wants to forbid.
#[derive(Debug, Clone, Default)]
pub(crate) struct ArchitectureTable {
    /// Each source crate and the dependencies it must not use.
    pub(crate) forbidden: BTreeMap<String, Vec<String>>,
    pub(crate) pending: PendingLedger<FileRule>,
    pub(crate) at: String,
    pub(crate) pending_at: String,
    pub(crate) unreadable: bool,
}

impl ArchitectureTable {
    /// Read optional `[[dependency_rules]]` entries. No table means no crate boundary rules.
    pub(crate) fn read(manifest: &Manifest, findings: &mut Vec<ArchitectureFinding>) -> Self {
        let key = KeyPath::from_segments(["dependency_rules"]);
        let pending_key = KeyPath::from_segments(["dependency_check_pending"]);
        let at = |key: &KeyPath| {
            manifest.origins(key).first().map_or_else(
                || "fun.toml".to_owned(),
                |origin| format!("{}:{}", origin.file().display(), origin.line()),
            )
        };
        let mut table = Self {
            at: at(&key),
            pending_at: at(&pending_key),
            ..Self::default()
        };
        let raw = match manifest.table::<Vec<RawRule>>(&key) {
            Ok(Some(raw)) => raw,
            Ok(None) => Vec::new(),
            Err(error) => {
                findings.push(ArchitectureFinding::InvalidTable {
                    at: table.at.clone(),
                    reason: error.to_string(),
                });
                table.unreadable = true;
                return table;
            }
        };
        table.forbidden = forbidden_edges(raw, &table.at, findings);
        let pending = match manifest.table::<Vec<RawPending>>(&pending_key) {
            Ok(Some(pending)) => pending,
            Ok(None) => Vec::new(),
            Err(error) => {
                findings.push(ArchitectureFinding::InvalidTable {
                    at: table.pending_at.clone(),
                    reason: error.to_string(),
                });
                Vec::new()
            }
        };
        let mut problems = Vec::new();
        table.pending = PendingLedger::read(pending, table.pending_at.clone(), &mut problems);
        findings.extend(
            problems
                .into_iter()
                .map(|reason| ArchitectureFinding::InvalidTable {
                    at: table.pending_at.clone(),
                    reason,
                }),
        );
        debug!(
            rules = table.forbidden.len(),
            pending = table.pending.entries().len(),
            "read dependency rules"
        );
        table
    }
}

/// Each named crate's forbidden dependencies, with a finding for every rule that cannot be read.
fn forbidden_edges(
    raw: Vec<RawRule>,
    at: &str,
    findings: &mut Vec<ArchitectureFinding>,
) -> BTreeMap<String, Vec<String>> {
    let mut forbidden: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut refuse = |reason| {
        findings.push(ArchitectureFinding::InvalidTable {
            at: at.to_owned(),
            reason,
        });
    };
    for (index, rule) in raw.into_iter().enumerate() {
        if rule.crate_name.trim().is_empty() || rule.crate_name.contains('/') {
            refuse(format!(
                "dependency rule {} has an invalid `crate` name",
                index + 1
            ));
            continue;
        }
        let targets = forbidden.entry(rule.crate_name.clone()).or_default();
        rule.must_not_depend_on
            .into_iter()
            .filter_map(|target| add_target(targets, target, &rule.crate_name))
            .for_each(&mut refuse);
    }
    forbidden
}

/// Adds `target` to a crate's forbidden dependencies, or says why it cannot be.
fn add_target(targets: &mut Vec<String>, target: String, crate_name: &str) -> Option<String> {
    if target.trim().is_empty() || target.contains('/') {
        return Some(format!(
            "dependency rule for `{crate_name}` has an invalid dependency name"
        ));
    }
    if targets.contains(&target) {
        return Some(format!(
            "`{target}` is listed more than once for `{crate_name}`"
        ));
    }
    targets.push(target);
    None
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    #[serde(rename = "crate")]
    crate_name: String,
    #[serde(default)]
    must_not_depend_on: Vec<String>,
}
