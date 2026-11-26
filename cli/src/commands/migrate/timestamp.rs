//! UTC timestamp formatting for migration file names.

use std::time::SystemTime;

/// The current UTC time as a `YYYYMMDDHHMMSS` prefix, `00000000000000` before the epoch.
pub(crate) fn migration_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_utc(secs)
}

fn format_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hour, minute, second) = (rem / 3_600, (rem % 3_600) / 60, rem % 60);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}{month:02}{day:02}{hour:02}{minute:02}{second:02}")
}

/// Howard Hinnant's `civil_from_days`, proleptic Gregorian.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_a_known_epoch_second() {
        assert_eq!(format_utc(1_609_459_200), "20210101000000");
    }

    #[test]
    fn formats_the_unix_epoch() {
        assert_eq!(format_utc(0), "19700101000000");
    }

    #[test]
    fn formats_time_of_day() {
        // 2000-02-29T13:37:45Z, a leap day.
        assert_eq!(format_utc(951_831_465), "20000229133745");
    }

    #[test]
    fn the_live_timestamp_is_fourteen_digits() {
        let ts = migration_timestamp();
        assert_eq!(ts.len(), 14);
        assert!(ts.chars().all(|c| c.is_ascii_digit()), "{ts}");
    }
}
