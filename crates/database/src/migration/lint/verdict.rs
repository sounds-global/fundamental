//! What a project's levels make of the lints that fired.

use fundamental_core::model::sql::{Level, Lint};

use super::{LintFinding, Lints};

/// The findings that block, of `found`, under `lints`; an acceptance passes only a warning.
pub(crate) fn blocking(
    found: Vec<LintFinding>,
    lints: &Lints,
    accepted: impl Fn(&LintFinding) -> bool,
) -> Vec<LintFinding> {
    found
        .into_iter()
        .filter_map(|finding| match lints.level(finding.lint) {
            Level::Allow => None,
            Level::Warn => (!accepted(&finding)).then_some(finding),
            Level::Refuse if accepted(&finding) => Some(LintFinding {
                at: format!(
                    "{}; it is accepted on the model, but this project refuses {}, and an \
                     acceptance cannot pass a refused lint",
                    finding.at, finding.lint
                ),
                ..finding
            }),
            Level::Refuse => Some(finding),
        })
        .collect()
}

/// Each acceptance whose lint fires nowhere on its table, refused as stale.
pub(crate) fn stale<'a>(
    accepted: impl IntoIterator<Item = (&'a str, Lint)>,
    found: &[LintFinding],
) -> Vec<LintFinding> {
    accepted
        .into_iter()
        .filter(|(table, lint)| {
            !found
                .iter()
                .any(|finding| finding.table == *table && finding.lint == *lint)
        })
        .map(|(table, lint)| {
            LintFinding::new(
                lint,
                table,
                table,
                format!(
                    "{table} accepts {lint}, which fires on it nowhere; take the acceptance off \
                     its model"
                ),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use fundamental_core::model::sql::{Level, Lint};

    use super::{blocking, stale};
    use crate::migration::lint::{LintFinding, Lints};

    fn hot() -> LintFinding {
        LintFinding::new(
            Lint::HotShard,
            "events",
            "events",
            "events leads with at ASC",
        )
    }

    #[test]
    fn a_warning_blocks_until_the_model_accepts_it() {
        let lints = Lints::default();

        assert_eq!(blocking(vec![hot()], &lints, |_| false), [hot()]);
        assert!(blocking(vec![hot()], &lints, |_| true).is_empty());
    }

    #[test]
    fn a_refused_lint_blocks_accepted_or_not_and_says_the_acceptance_cannot_pass_it() {
        let lints = Lints::default().with(Lint::HotShard, Level::Refuse);

        let blocked = blocking(vec![hot()], &lints, |_| true);

        assert_eq!(blocked.len(), 1);
        assert!(
            blocked[0]
                .at
                .contains("an acceptance cannot pass a refused lint")
        );
        assert_eq!(blocking(vec![hot()], &lints, |_| false), [hot()]);
    }

    #[test]
    fn an_allowed_lint_blocks_nothing() {
        let lints = Lints::default().with(Lint::HotShard, Level::Allow);

        assert!(blocking(vec![hot()], &lints, |_| false).is_empty());
    }

    #[test]
    fn an_acceptance_of_a_lint_that_fires_nowhere_on_its_table_is_stale() {
        let found = [hot()];

        assert!(stale([("events", Lint::HotShard)], &found).is_empty());
        let gone = stale(
            [("events", Lint::Backfill), ("orders", Lint::HotShard)],
            &found,
        );
        assert_eq!(gone.len(), 2);
        assert!(gone[0].at.contains("fires on it nowhere"));
    }
}
