/// The precision a wire stores an instant at, and so the precision an instant is bound at.
///
/// An instant is truncated to it before binding, or a nanosecond the store discarded would make every read of the
/// row fail to decrypt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampPrecision {
    /// Microseconds since the Unix epoch — the SQL wire.
    Microseconds,
    /// Milliseconds since the Unix epoch — the CQL wire.
    Milliseconds,
}
