//! Which of a bucket's stores holds an object.

/// The index of the store within a bucket that holds an object, recorded when it is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShardIndex(u8);

impl ShardIndex {
    /// The index `value`.
    #[must_use]
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    /// The index as a number, for a column.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}
