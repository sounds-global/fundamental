//! [`largest_bucket`] — how many draws of a simulated fleet land in its fullest time slot.

use std::collections::BTreeMap;
use std::time::Duration;

/// The number of `delays` in the fullest `width`-wide slot (the `seeded-entropy` feature).
///
/// ```
/// use core::time::Duration;
/// use fundamental_core::retry::largest_bucket;
///
/// let ms = Duration::from_millis;
/// assert_eq!(largest_bucket(&[ms(10), ms(90), ms(150)], ms(100)), 2);
/// ```
#[must_use]
pub fn largest_bucket(delays: &[Duration], width: Duration) -> usize {
    let width = width.as_nanos().max(1);
    let mut buckets = BTreeMap::<u128, usize>::new();
    for delay in delays {
        *buckets.entry(delay.as_nanos() / width).or_default() += 1;
    }
    buckets.values().copied().max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_fleet_has_no_peak_and_a_zero_width_is_one_nanosecond() {
        assert_eq!(largest_bucket(&[], Duration::from_millis(100)), 0);
        let same = [Duration::from_nanos(5); 3];
        assert_eq!(largest_bucket(&same, Duration::ZERO), 3);
    }
}
