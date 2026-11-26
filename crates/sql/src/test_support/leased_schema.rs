use core::fmt;

use super::release_leased_schema;

/// A schema this process holds the lease on, released when it goes out of scope; a missed release only delays.
pub struct LeasedSchema {
    name: String,
    /// A string, never a live pool; see [`release_leased_schema`].
    database_url: String,
}

impl LeasedSchema {
    /// Takes ownership of handing `schema` back to the pool in `database_url`, on every path out, a panic included.
    #[must_use]
    pub fn held(database_url: impl Into<String>, schema: impl Into<String>) -> Self {
        Self {
            name: schema.into(),
            database_url: database_url.into(),
        }
    }

    /// The schema name, for the pool and the statements that qualify it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The database this schema lives in.
    #[must_use]
    pub fn database_url(&self) -> &str {
        &self.database_url
    }
}

/// Prints the name and never the URL, which carries credentials.
impl fmt::Debug for LeasedSchema {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LeasedSchema")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Drop for LeasedSchema {
    fn drop(&mut self) {
        release_leased_schema(&self.database_url, &self.name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_debug_form_carries_the_name_and_not_the_url() {
        let leased = LeasedSchema::held(
            "postgresql://layer5:hunter2@localhost:5433/layer5",
            "layer5_test_1787809538320_5f7e48e6ef944263",
        );
        let printed = format!("{leased:?}");
        assert!(printed.contains("layer5_test_1787809538320_5f7e48e6ef944263"));
        assert!(
            !printed.contains("hunter2"),
            "a fixture that prints its URL is a fixture that logs a credential"
        );
        // Not dropped: this test is about the formatting, not the release.
        core::mem::forget(leased);
    }
}
