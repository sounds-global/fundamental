//! Database connection URL wrapper with password masking.

#[cfg(not(feature = "std"))]
use alloc::{
    format,
    string::{String, ToString},
};
use core::fmt;
#[cfg(feature = "std")]
use zeroize::Zeroize;

/// A database connection URL with password masking.
///
/// # Example
///
/// ```
/// use fundamental_core::secrets::DatabaseUrl;
///
/// let url = DatabaseUrl::new("postgresql://user:secret@localhost:5432/db".to_string());
///
/// // Debug shows masked URL
/// println!("{:?}", url);  // postgresql://user:***@localhost:5432/db
///
/// // Get the actual URL for connecting
/// let actual = url.expose();
/// ```
#[derive(Clone, Default)]
pub struct DatabaseUrl(String);

impl DatabaseUrl {
    /// Creates a new database URL.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self(url.into())
    }

    /// Exposes the actual database URL.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Consumes the wrapper and returns the actual URL.
    ///
    /// Bypasses zeroization: the caller becomes responsible for the secret.
    #[must_use]
    pub fn expose_owned(self) -> String {
        let mut this = core::mem::ManuallyDrop::new(self);
        // SAFETY: We're taking ownership and preventing double-drop via ManuallyDrop.
        // The inner String is valid and we're not accessing `this` after this.
        core::mem::take(&mut this.0)
    }

    /// Returns a masked version of the URL suitable for logging.
    #[must_use]
    pub fn masked(&self) -> String {
        mask_url_password(&self.0)
    }
}

impl fmt::Debug for DatabaseUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("DatabaseUrl").field(&self.masked()).finish()
    }
}

impl fmt::Display for DatabaseUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.masked())
    }
}

impl PartialEq for DatabaseUrl {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for DatabaseUrl {}

#[cfg(feature = "std")]
impl Drop for DatabaseUrl {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

// Serialization masks the password, so a config snapshot or log never leaks it.
impl serde::Serialize for DatabaseUrl {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.masked())
    }
}

impl<'de> serde::Deserialize<'de> for DatabaseUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer).map(DatabaseUrl)
    }
}

/// Masks the password in a database URL.
fn mask_url_password(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };

    let after_scheme = &url[scheme_end + 3..];

    let Some(at_pos) = after_scheme.find('@') else {
        return url.to_string(); // No credentials
    };

    let credentials = &after_scheme[..at_pos];

    let Some(colon_pos) = credentials.find(':') else {
        return url.to_string(); // No password
    };

    let scheme = &url[..scheme_end + 3];
    let user = &credentials[..colon_pos];
    let after_at = &after_scheme[at_pos..];

    format!("{scheme}{user}:***{after_at}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_masks_password() {
        let url = DatabaseUrl::new("postgresql://user:hunter2@localhost:5432/db".to_string());
        let json = serde_json::to_string(&url).expect("serialize");
        assert_eq!(json, "\"postgresql://user:***@localhost:5432/db\"");
        assert!(!json.contains("hunter2"));
    }

    #[test]
    fn deserialize_is_faithful() {
        let url: DatabaseUrl = serde_json::from_str("\"postgresql://user:hunter2@localhost/db\"")
            .expect("deserialize");
        assert_eq!(url.expose(), "postgresql://user:hunter2@localhost/db");
    }

    #[test]
    fn test_mask_url_password_various_schemes() {
        assert_eq!(
            mask_url_password("mysql://user:pass@host/db"),
            "mysql://user:***@host/db"
        );
        assert_eq!(
            mask_url_password("mongodb://user:pass@host:27017/db"),
            "mongodb://user:***@host:27017/db"
        );
        assert_eq!(
            mask_url_password("redis://user:pass@host:6379"),
            "redis://user:***@host:6379"
        );
    }
}
