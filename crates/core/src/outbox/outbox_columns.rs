//! [`OutboxColumns`] — the columns the framework owns on every outbox table.

/// The columns the framework owns on every outbox table, outside any model.
///
/// No model names them, so writing them cannot disturb what the row's encrypted values are sealed to.
#[derive(Debug, Clone, Copy)]
pub struct OutboxColumns;

impl OutboxColumns {
    /// The row's seal, written on every whole-row write and checked on every decode.
    pub const SEAL: &'static str = "outbox_seal";

    /// When a claim set the row aside; `NULL` while the row may be claimed.
    pub const SET_ASIDE_AT: &'static str = "outbox_set_aside_at";

    /// Why the row was set aside, as a [`RefusalKind`](super::RefusalKind) code.
    pub const SET_ASIDE_REASON: &'static str = "outbox_set_aside_reason";

    /// The seal a claim stores beside a set-aside; without a checking one it counts as unexplained.
    pub const SET_ASIDE_SEAL: &'static str = "outbox_set_aside_seal";

    /// Until when a claim that could not decode the row left it for another process.
    pub const SKIP_UNTIL: &'static str = "outbox_skip_until";
}

#[cfg(test)]
mod tests {
    use super::OutboxColumns;

    #[test]
    fn the_framework_columns_are_distinct_and_carry_the_outbox_prefix() {
        let columns = [
            OutboxColumns::SEAL,
            OutboxColumns::SET_ASIDE_AT,
            OutboxColumns::SET_ASIDE_REASON,
            OutboxColumns::SET_ASIDE_SEAL,
            OutboxColumns::SKIP_UNTIL,
        ];
        assert!(columns.iter().all(|column| column.starts_with("outbox_")));
        for (index, column) in columns.iter().enumerate() {
            assert!(
                columns.iter().skip(index + 1).all(|other| other != column),
                "{column} is named twice"
            );
        }
    }
}
