//! [`Resolve`]: the two acts that settle a failed migration.

use std::fmt;

/// How a failed migration is settled: run again, or marked done once the catalog shows it.
///
/// An enum, so there is no third act that marks a migration fixed on someone's word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolve {
    /// Run the step that failed and every step after it again.
    Retry,
    /// Record it as applied, because the live catalog already shows every effect it has.
    Applied,
}

impl Resolve {
    /// The word the ledger records it under, and the flag that asks for it without its `--`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Retry => "retry",
            Self::Applied => "applied",
        }
    }

    /// The act `word` names, or `None` for any other word.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "retry" => Some(Self::Retry),
            "applied" => Some(Self::Applied),
            _ => None,
        }
    }
}

impl fmt::Display for Resolve {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::Resolve;

    #[test]
    fn each_act_reads_back_from_its_word_and_nothing_else_is_one() {
        for act in [Resolve::Retry, Resolve::Applied] {
            assert_eq!(Resolve::parse(act.as_str()), Some(act));
        }
        for word in ["rolled-back", "skip", "Retry", ""] {
            assert_eq!(Resolve::parse(word), None, "{word:?}");
        }
    }
}
