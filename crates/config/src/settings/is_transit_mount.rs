//! [`is_transit_mount`]: whether a value can be a Transit engine's mount.

/// Whether `value` is 1–64 characters of `[a-z0-9-]`, the shape a Transit mount is held to.
#[must_use]
pub fn is_transit_mount(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::is_transit_mount;

    #[test]
    fn a_lower_case_name_is_a_mount_and_anything_else_is_not() {
        assert!(is_transit_mount("transit"));
        assert!(is_transit_mount("transit-eu-1"));
        for value in ["", "Transit", "transit/keys", "transit_1", &"t".repeat(65)] {
            assert!(!is_transit_mount(value), "{value:?}");
        }
    }
}
