//! [`DatabaseName`] — a database the local node holds for a project.

use crate::LocalError;

/// A database's name, `[a-z][a-z0-9_]{0,62}`, so it is written into SQL unquoted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DatabaseName(String);

impl DatabaseName {
    /// `raw`, if it is one plain name.
    ///
    /// # Errors
    ///
    /// [`LocalError::DatabaseNameInvalid`] (`LOCAL_024`) for anything else.
    pub fn parse(raw: &str) -> Result<Self, LocalError> {
        if plain(raw) {
            Ok(Self(raw.to_owned()))
        } else {
            Err(LocalError::DatabaseNameInvalid {
                name: raw.to_owned(),
            })
        }
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Whether `raw` is one plain SQL name, `[a-z][a-z0-9_]{0,62}`.
pub(crate) fn plain(raw: &str) -> bool {
    let mut chars = raw.chars();
    raw.len() <= 63
        && chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}
