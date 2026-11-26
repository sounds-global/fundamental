//! [`stored_moment`] — a moment as a `TIMESTAMPTZ` column keeps it.

use chrono::{DateTime, Duration, TimeZone, Utc};

/// `moment` as a `TIMESTAMPTZ` column keeps it: cut to whole microseconds toward the 2000 epoch; returned as is when out of range.
#[must_use]
pub fn stored_moment<Tz: TimeZone>(moment: DateTime<Tz>) -> DateTime<Tz> {
    let Some(epoch) = Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).single() else {
        return moment;
    };
    moment
        .with_timezone(&Utc)
        .signed_duration_since(epoch)
        .num_microseconds()
        .and_then(|micros| epoch.checked_add_signed(Duration::microseconds(micros)))
        .map_or(moment.clone(), |kept| {
            kept.with_timezone(&moment.timezone())
        })
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone as _, Utc};

    use super::stored_moment;

    #[test]
    fn a_moment_is_kept_to_the_microsecond_toward_the_epoch() {
        let at = |secs, nanos| Utc.timestamp_opt(secs, nanos).single().expect("a moment");
        assert_eq!(
            stored_moment(at(1_790_000_000, 123_456_789)),
            at(1_790_000_000, 123_456_000)
        );
        assert_eq!(stored_moment(at(1_790_000_000, 5)), at(1_790_000_000, 0));
        // Before 2000 the count is negative and is cut toward zero, so later in time.
        assert_eq!(
            stored_moment(at(900_000_000, 999_999_999)),
            at(900_000_001, 0)
        );
        assert_eq!(
            stored_moment(at(900_000_000, 123_456_789)),
            at(900_000_000, 123_457_000)
        );
    }
}
