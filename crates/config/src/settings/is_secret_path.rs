//! [`is_secret_path`]: whether a value names one place in a secret store.

/// Whether `value` is `/`-separated `[A-Za-z0-9._-]` segments, none empty, `.` or `..`.
#[must_use]
pub fn is_secret_path(value: &str) -> bool {
    !value.is_empty()
        && value.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
}

#[cfg(test)]
mod tests {
    use super::is_secret_path;

    #[test]
    fn a_path_of_plain_segments_is_one() {
        for value in ["secret", "nats/mijn", "database/creds/app/AbC.12-x_y"] {
            assert!(is_secret_path(value), "{value}");
        }
    }

    #[test]
    fn anything_that_could_name_another_place_is_not() {
        for value in [
            "",
            "/",
            "nats/../other",
            "nats//mijn",
            "nats/./mijn",
            "nats/mi jn",
            "nats?list=true",
            "/nats",
        ] {
            assert!(!is_secret_path(value), "{value:?}");
        }
    }
}
