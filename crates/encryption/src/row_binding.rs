use crate::fns::length_prefixed;

/// The envelope format of a field sealed to its row. Every envelope this crate produces carries
/// it, and decryption refuses any other.
pub const ROW_BOUND_FORMAT: u8 = 2;

/// The label every row binding's associated data begins with, so it can never authenticate a
/// keyring token or any other structure this crate frames alike.
const LABEL: &[u8] = b"fundamental model field";

/// Where a sealed model field lives: its table, its column, and every non-`NULL` non-secret
/// column of its row, ordered by name; opening under any other binding fails authentication.
#[derive(Debug, Clone, Copy)]
pub struct RowBinding<'a> {
    table: &'a str,
    column: &'a str,
    parts: &'a [(&'a str, Option<Vec<u8>>)],
}

impl<'a> RowBinding<'a> {
    /// The binding of `column` in `table`, over its row's bound columns as `(name, bytes)`, the
    /// bytes `None` for a `NULL` value.
    #[must_use]
    pub fn new(table: &'a str, column: &'a str, parts: &'a [(&'a str, Option<Vec<u8>>)]) -> Self {
        Self {
            table,
            column,
            parts,
        }
    }

    /// The associated data a field under this binding is sealed with, for a key declared for
    /// `purpose` (empty when the key was not configured with one).
    pub(crate) fn associated_data(&self, purpose: &str) -> Vec<u8> {
        let format = [ROW_BOUND_FORMAT];
        // Sorted on the whole pair so a column named twice still encodes deterministically.
        let mut present: Vec<(&[u8], &[u8])> = self
            .parts
            .iter()
            .filter_map(|(name, bytes)| bytes.as_deref().map(|bytes| (name.as_bytes(), bytes)))
            .collect();
        present.sort_unstable();
        let header: [&[u8]; 5] = [
            LABEL,
            &format,
            purpose.as_bytes(),
            self.table.as_bytes(),
            self.column.as_bytes(),
        ];
        let framed: Vec<&[u8]> = header
            .into_iter()
            .chain(present.into_iter().flat_map(|(name, bytes)| [name, bytes]))
            .collect();
        length_prefixed(framed.into_iter())
    }
}

#[cfg(test)]
mod tests {
    use super::RowBinding;

    fn ad(table: &str, column: &str, parts: &[(&str, Option<Vec<u8>>)], purpose: &str) -> Vec<u8> {
        RowBinding::new(table, column, parts).associated_data(purpose)
    }

    fn part(bytes: &[u8]) -> Option<Vec<u8>> {
        Some(bytes.to_vec())
    }

    #[test]
    fn every_element_of_the_binding_changes_the_associated_data() {
        let parts = [("zone", part(b"\x01eu")), ("id", part(b"\x02id"))];
        let base = ad("attribute_values", "value", &parts, "pii");
        let other_value = [("zone", part(b"\x01eu")), ("id", part(b"\x02other"))];
        for different in [
            ad("audit_records", "value", &parts, "pii"),
            ad("attribute_values", "salt", &parts, "pii"),
            ad("attribute_values", "value", &other_value, "pii"),
            ad("attribute_values", "value", &parts, "mail"),
        ] {
            assert_ne!(base, different);
        }
    }

    #[test]
    fn a_part_boundary_cannot_be_moved() {
        let split_early = [("a", part(b"a")), ("b", part(b"bc"))];
        let split_late = [("a", part(b"ab")), ("b", part(b"c"))];
        assert_ne!(
            ad("t", "c", &split_early, ""),
            ad("t", "c", &split_late, "")
        );
    }

    #[test]
    fn the_order_a_model_declares_its_columns_in_does_not_matter() {
        let declared = [("owner", part(b"\x01a")), ("confirmed_at", part(b"\x06t"))];
        let reordered = [("confirmed_at", part(b"\x06t")), ("owner", part(b"\x01a"))];
        assert_eq!(ad("t", "c", &declared, ""), ad("t", "c", &reordered, ""));
    }

    #[test]
    fn a_nullable_column_added_later_leaves_an_older_row_readable() {
        let before = [("owner", part(b"\x01a"))];
        let after = [("owner", part(b"\x01a")), ("reviewed_at", None)];
        assert_eq!(ad("t", "c", &before, ""), ad("t", "c", &after, ""));
    }

    #[test]
    fn nulling_or_filling_a_bound_column_changes_the_binding() {
        let filled = [("owner", part(b"\x01a")), ("note", part(b"\x01"))];
        let nulled = [("owner", part(b"\x01a")), ("note", None)];
        assert_ne!(ad("t", "c", &filled, ""), ad("t", "c", &nulled, ""));
    }

    #[test]
    fn a_value_under_another_column_name_binds_differently() {
        let as_written = [
            ("valid_from", part(b"\x061")),
            ("valid_until", part(b"\x062")),
        ];
        let swapped = [
            ("valid_from", part(b"\x062")),
            ("valid_until", part(b"\x061")),
        ];
        assert_ne!(ad("t", "c", &as_written, ""), ad("t", "c", &swapped, ""));
    }
}
