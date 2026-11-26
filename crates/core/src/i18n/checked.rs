//! [`Checked`] — who confirmed a text still fits its other language, and when.

#[cfg(not(feature = "std"))]
use alloc::string::String;

use serde::{Deserialize, Serialize};

/// Who confirmed a text still fits its other language, and when.
///
/// ```
/// use fundamental_core::i18n::Checked;
///
/// let checked = Checked::new("ann@example.com", 1_763_900_000_000);
/// assert_eq!(checked.by(), "ann@example.com");
/// assert_eq!(checked.at_millis(), 1_763_900_000_000);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checked {
    by: String,
    at: i64,
}

impl Checked {
    /// Confirmed by `by` at `at_millis`, milliseconds since the Unix epoch in UTC.
    pub fn new(by: impl Into<String>, at_millis: i64) -> Self {
        Self {
            by: by.into(),
            at: at_millis,
        }
    }

    /// Who confirmed it, as the app names a person.
    pub fn by(&self) -> &str {
        &self.by
    }

    /// When, in milliseconds since the Unix epoch, in UTC.
    pub fn at_millis(&self) -> i64 {
        self.at
    }

    /// When, as an instant; `None` past what an instant holds.
    #[cfg(feature = "chrono")]
    pub fn at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        chrono::DateTime::from_timestamp_millis(self.at)
    }
}
