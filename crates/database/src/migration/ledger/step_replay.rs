//! [`StepReplay`]: one migration replayed into an empty schema a step at a time.

use crate::migration::generate::Snapshot;

/// One migration replayed into an empty schema a step at a time: the catalog the migrations
/// before it leave, then the catalog after each of its steps, with the step's statement.
///
/// What [`CatalogProof`](super::CatalogProof) is checked against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepReplay {
    before: Snapshot,
    steps: Vec<(String, Snapshot)>,
}

impl StepReplay {
    /// A replay that starts from `before`, the catalog the migrations before this one leave.
    #[must_use]
    pub fn new(before: Snapshot) -> Self {
        Self {
            before,
            steps: Vec::new(),
        }
    }

    /// Adds the next step: its `statement`, and the catalog `after` it ran.
    pub fn step(&mut self, statement: impl Into<String>, after: Snapshot) {
        self.steps.push((statement.into(), after));
    }

    /// The catalog before the migration's first step.
    #[must_use]
    pub fn before(&self) -> &Snapshot {
        &self.before
    }

    /// The catalog after the migration's last step: [`before`](Self::before) when it has none.
    #[must_use]
    pub fn after(&self) -> &Snapshot {
        self.steps.last().map_or(&self.before, |(_, after)| after)
    }

    /// Each step counting from one, with its statement, and whether it changed the catalog.
    pub(super) fn changes(&self) -> impl Iterator<Item = (usize, &str, bool)> {
        let befores = std::iter::once(&self.before).chain(self.steps.iter().map(|(_, s)| s));
        self.steps
            .iter()
            .zip(befores)
            .enumerate()
            .map(|(index, ((statement, after), before))| {
                (
                    index + 1,
                    statement.as_str(),
                    !before.differences(after).is_empty(),
                )
            })
    }

    /// How many steps it has.
    #[must_use]
    pub fn total(&self) -> usize {
        self.steps.len()
    }
}
