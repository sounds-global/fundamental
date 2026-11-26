//! The [`OrderNulls`] null-placement preference.

/// Nulls ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderNulls {
    /// Nulls appear first.
    First,
    /// Nulls appear last.
    Last,
}
