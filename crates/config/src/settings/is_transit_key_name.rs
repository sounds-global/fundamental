//! [`is_transit_key_name`]: whether a value can be a Transit key's name.

/// Whether `value` is 1–128 characters of `[a-z0-9_-]`.
#[must_use]
pub fn is_transit_key_name(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::is_transit_key_name;

    #[test]
    fn a_lower_case_name_is_a_key_and_anything_else_is_not() {
        assert!(is_transit_key_name("nats-orders"));
        assert!(is_transit_key_name("nats_orders_1"));
        for value in ["", "Nats", "nats/orders", "nats.orders", &"k".repeat(129)] {
            assert!(!is_transit_key_name(value), "{value:?}");
        }
    }
}
