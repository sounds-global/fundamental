/// Whether `value` is safe to inline into SQL: plain identifier characters only, as a store name cannot be bound.
pub(super) fn is_inlinable(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::is_inlinable;

    #[test]
    fn plain_identifiers_are_inlinable() {
        for value in ["t_", "t_1754500000000_", "authz_", "abc123"] {
            assert!(is_inlinable(value), "{value}");
        }
    }

    #[test]
    fn anything_that_could_end_the_literal_is_not() {
        for value in ["", "t'", "t; DROP DATABASE prod; --", "t%", "t\"", "t\\"] {
            assert!(!is_inlinable(value), "{value:?} must be refused");
        }
    }
}
