//! [`IsolationRule`], one way a test reaches beyond its own rows.

use std::fmt;

use fundamental_core::Variant as _;
use fundamental_macros::Variant;

/// One way a test running beside others reaches beyond its own rows; its value is a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Variant)]
pub enum IsolationRule {
    /// A database role leased while other tests run, breaking their catalog snapshots.
    #[variant(value = "ISO_001")]
    LeaseBesideOthers,
    /// A lease the test never revokes, so its `DROP ROLE` lands in a later test's run.
    #[variant(value = "ISO_002")]
    LeaseNotRevoked,
    /// A fixture that changes the schema while other tests run.
    #[variant(value = "ISO_003")]
    SchemaChange,
    /// A job drain or sweep that fires every due job, the ones other tests armed included.
    #[variant(value = "ISO_004")]
    UnconfinedDrain,
    /// A sleep for a fixed time: too short on a loaded machine, too long on every other.
    #[variant(value = "ISO_005")]
    FixedSleep,
    /// The outcome of an `eventually` dropped, so a condition that never held passes.
    #[variant(value = "ISO_006")]
    DiscardedWait,
}

impl IsolationRule {
    /// What to do instead.
    #[must_use]
    pub const fn remedy(self) -> &'static str {
        match self {
            Self::LeaseBesideOthers => {
                "connect on the credentials the stack leased once, or run the test alone"
            }
            Self::LeaseNotRevoked => "close the connections before the test ends",
            Self::SchemaChange => "keep the schema to the migrations, or run the test alone",
            Self::UnconfinedDrain => {
                "confine the drain to what the test armed, or name it as a relay whose effect is the same whoever fires it"
            }
            Self::FixedSleep => {
                "wait for the condition with `eventually`, drive the job or the clock, or sleep for the project's named race window"
            }
            Self::DiscardedWait => "return or assert what `eventually` answered",
        }
    }
}

impl fmt::Display for IsolationRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.value())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use fundamental_core::Variant;

    use super::IsolationRule;

    #[test]
    fn each_rule_has_its_own_code_and_remedy() {
        let rules = IsolationRule::OPTIONS;
        let codes: BTreeSet<&str> = rules.iter().map(Variant::value).collect();
        assert_eq!(codes.len(), rules.len());
        for rule in rules {
            assert!(rule.value().starts_with("ISO_"), "{rule}");
            assert!(!rule.remedy().is_empty(), "{rule}");
        }
    }
}
