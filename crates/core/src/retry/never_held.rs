//! [`NeverHeld`] — a condition [`eventually`](super::eventually) waited for and never saw.

use core::fmt;
use core::time::Duration;

/// The condition named `what` did not hold within its deadline, after `asked` probes.
#[must_use = "a condition that never held is the failure; return or assert it"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeverHeld {
    what: &'static str,
    within: Duration,
    asked: u32,
}

impl NeverHeld {
    pub(super) const fn new(what: &'static str, within: Duration, asked: u32) -> Self {
        Self {
            what,
            within,
            asked,
        }
    }

    /// The condition, as the caller named it.
    #[must_use]
    pub const fn what(&self) -> &'static str {
        self.what
    }

    /// The deadline it was given.
    #[must_use]
    pub const fn within(&self) -> Duration {
        self.within
    }

    /// How many times it was asked.
    #[must_use]
    pub const fn asked(&self) -> u32 {
        self.asked
    }
}

impl fmt::Display for NeverHeld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} never held within {:?} ({} probes)",
            self.what, self.within, self.asked
        )
    }
}

impl core::error::Error for NeverHeld {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_names_the_condition_its_deadline_and_its_probes() {
        let never = NeverHeld::new("the invoice is paid", Duration::from_secs(2), 9);
        assert_eq!(
            never.to_string(),
            "the invoice is paid never held within 2s (9 probes)"
        );
        assert_eq!(never.what(), "the invoice is paid");
        assert_eq!(never.within(), Duration::from_secs(2));
        assert_eq!(never.asked(), 9);
    }
}
