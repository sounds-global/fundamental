//! [`stale_cutoff`] — the bound a sweep may remove below, expressed as a name.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::EphemeralName;

/// How old a leftover store must be before a sweep may remove it.
///
/// Long enough that a store in use by a concurrent or paused suite always survives.
pub const STALE_AFTER: Duration = Duration::from_secs(6 * 60 * 60);

/// The exclusive upper bound, as a name, of what a sweep may remove under `prefix`.
///
/// A name in the same shape as [`EphemeralName`], so the sweep is one predicate the engine
/// evaluates itself:
///
/// ```sql
/// WHERE name LIKE 't\_%' AND name < 't_1754500000000_'
/// ```
///
/// `prefix` includes its trailing separator. When stores were minted under many prefixes,
/// use [`stale_cutoff_millis`] with [`EphemeralName::stamp_of`] instead.
#[must_use]
pub fn stale_cutoff(prefix: &str) -> String {
    // The trailing separator excludes the whole cutoff millisecond: no minted name is a prefix of it.
    format!(
        "{prefix}{cutoff:0width$}_",
        cutoff = stale_cutoff_millis(),
        width = EphemeralName::STAMP_DIGITS
    )
}

/// The same bound as a number, for a sweep that reads stamps rather than compares names.
#[must_use]
pub fn stale_cutoff_millis() -> u64 {
    // A clock that cannot answer yields zero, which sweeps nothing: the safe direction.
    SystemTime::now()
        .checked_sub(STALE_AFTER)
        .and_then(|instant| instant.duration_since(UNIX_EPOCH).ok())
        .and_then(|since| u64::try_from(since.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_freshly_minted_name_sorts_after_the_cutoff() {
        // What another binary is using right now must not be removable.
        let name = EphemeralName::mint("t_", 63).expect("valid prefix");
        assert!(
            name.as_str() > stale_cutoff("t_").as_str(),
            "{name} would be swept while still in use"
        );
    }

    #[test]
    fn a_freshly_minted_stamp_is_after_the_numeric_cutoff() {
        let name = EphemeralName::mint("layer5_test_", 63).expect("valid prefix");
        let stamp = EphemeralName::stamp_of(name.as_str()).expect("recognised");
        assert!(
            stamp > stale_cutoff_millis(),
            "{name} would be swept while in use"
        );
    }

    #[test]
    fn a_name_older_than_the_window_sorts_before_the_cutoff() {
        let long_ago = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_millis()
            - (STALE_AFTER.as_millis() + 60_000);
        let name = format!("t_{long_ago:013}_0123456789abcdef");
        assert!(name.as_str() < stale_cutoff("t_").as_str(), "{name}");
        assert!(
            EphemeralName::stamp_of(&name).expect("recognised") < stale_cutoff_millis(),
            "{name}"
        );
    }

    #[test]
    fn the_cutoff_carries_the_prefix_it_was_asked_for() {
        // Without the prefix the bound would match a hand-made database that sorts low.
        assert!(stale_cutoff("ks_").starts_with("ks_"));
    }

    #[test]
    fn lexicographic_order_is_chronological_order_across_a_digit_boundary() {
        // Zero-padding is what makes the string comparison sound.
        let earlier = format!("t_{:013}_aaaa", 999_999_999_999_u64);
        let later = format!("t_{:013}_aaaa", 1_000_000_000_000_u64);
        assert!(earlier < later, "{earlier} must sort before {later}");
    }
}
