//! Display adapter for `Option<T>` fields interpolated in `#[error(...)]`.

use core::fmt::{self, Debug, Display};

/// Renders an optional field inside a derived `Error` message: the value, or `None(<field>)`.
pub struct OptDisplay<'a, T> {
    value: &'a Option<T>,
    field: &'a str,
}

impl<'a, T> OptDisplay<'a, T> {
    /// Wraps an optional field together with its declared name.
    pub fn new(value: &'a Option<T>, field: &'a str) -> Self {
        Self { value, field }
    }
}

impl<T: Display> Display for OptDisplay<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            Some(v) => Display::fmt(v, f),
            None => write!(f, "None({})", self.field),
        }
    }
}

/// Mirrors `Display`, so `{field:?}` renders the inner value too.
impl<T: Display> Debug for OptDisplay<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}
