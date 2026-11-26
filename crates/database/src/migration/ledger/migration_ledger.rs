//! [`MigrationLedger`]: every migration on disk, and where each stands in one database.

use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Utc};

use super::{FailedStep, LedgerState, Resolved, StepRecord, StepState};
use crate::migration::split_statements;

/// One migration on disk and where it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    name: String,
    state: LedgerState,
}

impl LedgerEntry {
    /// Its ledger name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Where it stands.
    #[must_use]
    pub fn state(&self) -> &LedgerState {
        &self.state
    }
}

/// Every migration on disk, in the order they apply, each with where it stands: what
/// `migrate status` prints.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MigrationLedger {
    entries: Vec<LedgerEntry>,
}

impl MigrationLedger {
    /// The ledger of `files`, each `(ledger name, text)` in apply order, read from what the
    /// database recorded.
    ///
    /// A migration recorded as applied is applied, whatever its step rows say.
    #[must_use]
    pub fn read(
        files: &[(&str, &str)],
        applied: &BTreeMap<String, DateTime<Utc>>,
        steps: &[StepRecord],
        resolutions: &[Resolved],
    ) -> Self {
        let mut in_the_way: Option<String> = None;
        let entries = files
            .iter()
            .map(|(name, text)| {
                let state = if let Some(at) = applied.get(*name) {
                    LedgerState::Applied {
                        at: *at,
                        resolved: resolutions
                            .iter()
                            .filter(|resolved| resolved.name == *name)
                            .max_by_key(|resolved| resolved.at)
                            .cloned(),
                    }
                } else if let Some(state) = unfinished(name, text, steps) {
                    in_the_way.get_or_insert_with(|| (*name).to_owned());
                    state
                } else if let Some(behind) = &in_the_way {
                    LedgerState::Blocked {
                        behind: behind.clone(),
                    }
                } else {
                    LedgerState::Pending
                };
                LedgerEntry {
                    name: (*name).to_owned(),
                    state,
                }
            })
            .collect();
        Self { entries }
    }

    /// Every migration on disk, in the order they apply.
    #[must_use]
    pub fn entries(&self) -> &[LedgerEntry] {
        &self.entries
    }

    /// The first migration that failed and is not settled, with where it stopped.
    #[must_use]
    pub fn unresolved(&self) -> Option<(&str, &FailedStep)> {
        self.entries.iter().find_map(|entry| match &entry.state {
            LedgerState::Failed(step) => Some((entry.name.as_str(), step)),
            _ => None,
        })
    }

    /// Whether every migration on disk is applied.
    #[must_use]
    pub fn is_applied(&self) -> bool {
        self.entries
            .iter()
            .all(|entry| matches!(entry.state, LedgerState::Applied { .. }))
    }
}

fn unfinished(name: &str, text: &str, steps: &[StepRecord]) -> Option<LedgerState> {
    let mine = || steps.iter().filter(move |record| record.name == name);
    if let Some(failed) = mine().find(|record| record.state == StepState::Failed) {
        let statement = split_statements(text)
            .get(failed.step.saturating_sub(1))
            .cloned()
            .unwrap_or_else(|| "(the file on disk no longer has this step)".to_owned());
        return Some(LedgerState::Failed(FailedStep {
            step: failed.step,
            total: failed.total,
            statement,
            code: failed.code.clone(),
            error: failed.error.clone().unwrap_or_default(),
            answer_lost: failed.answer_lost,
            at: failed.at,
        }));
    }
    mine()
        .find(|record| record.state == StepState::Running)
        .map(|running| LedgerState::Running {
            step: running.step,
            total: running.total,
            since: running.at,
        })
}

impl fmt::Display for MigrationLedger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.entries.is_empty() {
            return writeln!(f, "  no migrations");
        }
        let width = self
            .entries
            .iter()
            .map(|entry| entry.name.len())
            .max()
            .unwrap_or(0);
        for entry in &self.entries {
            write_entry(f, &entry.name, width, &entry.state)?;
        }
        Ok(())
    }
}

fn write_entry(
    f: &mut fmt::Formatter<'_>,
    name: &str,
    width: usize,
    state: &LedgerState,
) -> fmt::Result {
    match state {
        LedgerState::Applied { at, resolved } => {
            write!(f, "  {name:<width$}   applied {}", minute(at))?;
            if let Some(resolved) = resolved {
                write!(
                    f,
                    ", resolved ({}) by {}: {}",
                    resolved.act, resolved.by, resolved.reason
                )?;
            }
            writeln!(f)
        }
        LedgerState::Failed(step) => write_failed(f, name, width, step),
        LedgerState::Running { step, total, since } => writeln!(
            f,
            "  {name:<width$}   running step {step} of {total} since {}",
            minute(since)
        ),
        LedgerState::Pending => writeln!(f, "  {name:<width$}   pending"),
        LedgerState::Blocked { behind } => {
            writeln!(f, "  {name:<width$}   pending, after {behind}")
        }
    }
}

fn write_failed(
    f: &mut fmt::Formatter<'_>,
    name: &str,
    width: usize,
    step: &FailedStep,
) -> fmt::Result {
    writeln!(
        f,
        "  {name:<width$}   failed at step {} of {} ({})",
        step.step,
        step.total,
        minute(&step.at)
    )?;
    writeln!(f, "      {}", step.statement.trim())?;
    match &step.code {
        Some(code) => writeln!(f, "      {code}: {}", step.error)?,
        None => writeln!(f, "      {}", step.error)?,
    }
    if step.answer_lost {
        writeln!(
            f,
            "      the answer was lost: whether it applied is unknown"
        )?;
    }
    writeln!(
        f,
        "      resolve: migrate resolve {name} --retry | --applied --reason <text>"
    )
}

fn minute(at: &DateTime<Utc>) -> String {
    at.format("%Y-%m-%d %H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{DateTime, TimeZone as _, Utc};

    use super::MigrationLedger;
    use crate::migration::ledger::{LedgerState, Resolve, Resolved, StepRecord, StepState};

    fn at(day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, day, hour, minute, 0)
            .single()
            .expect("a time")
    }

    const FILES: &[(&str, &str)] = &[
        (
            "0006_add_invoice_notes",
            "ALTER TABLE invoices ADD COLUMN IF NOT EXISTS notes TEXT;",
        ),
        (
            "0007_index_orders",
            "ALTER TABLE orders ADD COLUMN IF NOT EXISTS customer_id UUID;\n\
             CREATE INDEX IF NOT EXISTS orders_by_customer ON orders (customer_id HASH);\n\
             ANALYZE orders;",
        ),
        (
            "0008_orders_paid_at",
            "ALTER TABLE orders ADD COLUMN IF NOT EXISTS paid_at TIMESTAMPTZ;",
        ),
    ];

    fn step(step: usize, state: StepState) -> StepRecord {
        StepRecord {
            name: "0007_index_orders".to_owned(),
            step,
            total: 3,
            statement_hash: String::new(),
            state,
            code: (state == StepState::Failed).then(|| "57014".to_owned()),
            error: (state == StepState::Failed)
                .then(|| "canceling statement due to statement timeout".to_owned()),
            answer_lost: false,
            at: at(26, 9, 14),
        }
    }

    fn applied() -> BTreeMap<String, DateTime<Utc>> {
        BTreeMap::from([("0006_add_invoice_notes".to_owned(), at(20, 14, 2))])
    }

    #[test]
    fn a_failed_step_says_where_and_what_is_after_it_waits() {
        let ledger = MigrationLedger::read(
            FILES,
            &applied(),
            &[step(1, StepState::Done), step(2, StepState::Failed)],
            &[],
        );
        assert_eq!(
            ledger.to_string(),
            "  0006_add_invoice_notes   applied 2026-11-20 14:02\n\
             \x20 0007_index_orders        failed at step 2 of 3 (2026-11-26 09:14)\n\
             \x20     CREATE INDEX IF NOT EXISTS orders_by_customer ON orders (customer_id HASH);\n\
             \x20     57014: canceling statement due to statement timeout\n\
             \x20     resolve: migrate resolve 0007_index_orders --retry | --applied --reason <text>\n\
             \x20 0008_orders_paid_at      pending, after 0007_index_orders\n"
        );
        let (name, failed) = ledger.unresolved().expect("one is unresolved");
        assert_eq!(
            (name, failed.step, failed.total),
            ("0007_index_orders", 2, 3)
        );
        assert!(!ledger.is_applied());
    }

    #[test]
    fn an_applied_migration_is_applied_whatever_its_steps_say_and_names_its_resolution() {
        let mut applied = applied();
        applied.insert("0007_index_orders".to_owned(), at(26, 22, 0));
        let resolved = Resolved {
            name: "0007_index_orders".to_owned(),
            act: Resolve::Retry,
            by: "angelo".to_owned(),
            reason: "timeout at peak; rerun at night".to_owned(),
            login: Some("app_migrator".to_owned()),
            step: 2,
            code: Some("57014".to_owned()),
            at: at(26, 22, 0),
        };
        let ledger =
            MigrationLedger::read(FILES, &applied, &[step(2, StepState::Failed)], &[resolved]);
        let text = ledger.to_string();
        assert!(
            text.contains(
                "0007_index_orders        applied 2026-11-26 22:00, resolved (retry) by angelo: \
                 timeout at peak; rerun at night"
            ),
            "{text}"
        );
        assert!(ledger.unresolved().is_none());
        assert!(matches!(ledger.entries()[2].state(), LedgerState::Pending));
    }

    #[test]
    fn a_step_still_running_is_shown_running_and_holds_back_what_follows() {
        let ledger = MigrationLedger::read(FILES, &applied(), &[step(2, StepState::Running)], &[]);
        assert!(matches!(
            ledger.entries()[1].state(),
            LedgerState::Running {
                step: 2,
                total: 3,
                ..
            }
        ));
        assert!(matches!(
            ledger.entries()[2].state(),
            LedgerState::Blocked { .. }
        ));
        assert!(ledger.unresolved().is_none());
    }

    #[test]
    fn no_files_is_said_rather_than_printed_as_nothing() {
        assert_eq!(
            MigrationLedger::read(&[], &BTreeMap::new(), &[], &[]).to_string(),
            "  no migrations\n"
        );
    }
}
