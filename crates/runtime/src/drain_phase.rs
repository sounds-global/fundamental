//! [`DrainPhase`]: when a surface is stopped relative to the others.

/// When a surface is stopped, relative to every other surface; registration order decides within a phase.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum DrainPhase {
    /// Accepts new outside work (an HTTP API, a socket server, a subscriber); stopped first.
    Intake,

    /// Does the work (outboxes, periodic tasks, relays); the default.
    #[default]
    Work,

    /// Reports on the process (the admin listener); stopped last, so `/ready` answers through the drain.
    Observability,
}

impl DrainPhase {
    /// The phases in the order the runtime drains them.
    pub const ORDER: [Self; 3] = [Self::Intake, Self::Work, Self::Observability];

    /// A short, stable label for logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Intake => "intake",
            Self::Work => "work",
            Self::Observability => "observability",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_work() {
        assert_eq!(DrainPhase::default(), DrainPhase::Work);
    }

    #[test]
    fn order_runs_intake_then_work_then_observability() {
        let mut sorted = DrainPhase::ORDER;
        sorted.sort_unstable();
        assert_eq!(sorted, DrainPhase::ORDER);
        assert!(DrainPhase::Intake < DrainPhase::Work);
        assert!(DrainPhase::Work < DrainPhase::Observability);
    }

    #[test]
    fn every_phase_has_a_distinct_label() {
        let labels: Vec<&str> = DrainPhase::ORDER.iter().map(|p| p.as_str()).collect();
        assert_eq!(labels, ["intake", "work", "observability"]);
    }
}
