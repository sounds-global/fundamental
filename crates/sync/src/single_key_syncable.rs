//! [`SingleKeySyncable`] — the marker for a syncable table addressed by one column.

use crate::ServerSyncable;

/// A [`ServerSyncable`] whose key is exactly one column, a bound on seams that cannot address a
/// composite key.
pub trait SingleKeySyncable: ServerSyncable {
    /// The one column that addresses a row; always `KEY_COLUMNS[0]`.
    const KEY_COLUMN: &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OneColumn;
    struct TwoColumns;

    impl ServerSyncable for OneColumn {
        const TABLE: &'static str = "ones";
        const KEY_COLUMNS: &'static [&'static str] = &["id"];
        const PARTITION_COLUMNS: &'static [&'static str] = &["id"];
        const BIDIRECTIONAL: bool = false;
        fn data_columns() -> &'static [&'static str] {
            &["body"]
        }
    }

    impl SingleKeySyncable for OneColumn {
        const KEY_COLUMN: &'static str = "id";
    }

    impl ServerSyncable for TwoColumns {
        const TABLE: &'static str = "twos";
        const KEY_COLUMNS: &'static [&'static str] = &["zone", "id"];
        const PARTITION_COLUMNS: &'static [&'static str] = &["zone"];
        const BIDIRECTIONAL: bool = false;
        fn data_columns() -> &'static [&'static str] {
            &["body"]
        }
    }

    fn only_single<M: SingleKeySyncable>() -> usize {
        M::KEY_COLUMNS.len()
    }

    #[test]
    fn the_marker_admits_a_one_column_key() {
        assert_eq!(only_single::<OneColumn>(), 1);
    }

    #[test]
    fn the_named_column_is_the_key() {
        assert_eq!(
            <OneColumn as SingleKeySyncable>::KEY_COLUMN,
            <OneColumn as ServerSyncable>::KEY_COLUMNS[0]
        );
    }

    #[test]
    fn a_composite_key_is_never_one_column() {
        assert_eq!(<TwoColumns as ServerSyncable>::KEY_COLUMNS.len(), 2);
    }
}
