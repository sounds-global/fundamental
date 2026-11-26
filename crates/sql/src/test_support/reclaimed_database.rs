use core::fmt;

use fundamental_db::ephemeral::{EphemeralName, EphemeralNameError};

use super::drop_test_database;

/// A SQL database identifier's limit.
const MAX_DATABASE_NAME: usize = 63;

/// A minted database name whose reclamation is guaranteed before the database exists.
///
/// # Example
///
/// ```no_run
/// # use fundamental_sql::test_support::ReclaimedDatabase;
/// # async fn provision(bootstrap_url: &str) -> Result<(), Box<dyn std::error::Error>> {
/// // Nothing exists on the node yet, and the name is already accounted for.
/// let reclaimed = ReclaimedDatabase::claim(bootstrap_url, "layer5_test_")?;
///
/// // Every failure from here on reclaims, including a panic.
/// create_database(reclaimed.name()).await?;
/// migrate(reclaimed.name()).await?;
/// # Ok(())
/// # }
/// # async fn create_database(_: &str) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
/// # async fn migrate(_: &str) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
/// ```
pub struct ReclaimedDatabase {
    name: EphemeralName,
    /// A string, never a live pool; see [`drop_test_database`].
    bootstrap_url: String,
}

impl ReclaimedDatabase {
    /// Mints a name and takes ownership of removing whatever is created under it, from `bootstrap_url`.
    ///
    /// # Errors
    /// Returns [`EphemeralNameError`] if the prefix is not a plain identifier, does not
    /// end with `_`, or would mint a name longer than a SQL database allows.
    pub fn claim(
        bootstrap_url: impl Into<String>,
        prefix: &str,
    ) -> Result<Self, EphemeralNameError> {
        Ok(Self {
            name: EphemeralName::mint(prefix, MAX_DATABASE_NAME)?,
            bootstrap_url: bootstrap_url.into(),
        })
    }

    /// The database name, for the DDL that is about to create it.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// The URL the create and the drop both run from.
    #[must_use]
    pub fn bootstrap_url(&self) -> &str {
        &self.bootstrap_url
    }
}

/// Prints the name and never the URL, which carries credentials.
impl fmt::Debug for ReclaimedDatabase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReclaimedDatabase")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Drop for ReclaimedDatabase {
    fn drop(&mut self) {
        drop_test_database(&self.bootstrap_url, self.name.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_claim_names_a_database_before_one_exists() {
        let reclaimed = ReclaimedDatabase::claim("postgresql://localhost/postgres", "layer5_test_")
            .expect("a plain prefix is accepted");
        assert!(reclaimed.name().starts_with("layer5_test_"));
        assert!(
            EphemeralName::stamp_of(reclaimed.name()).is_some(),
            "{} must be sweepable",
            reclaimed.name()
        );
    }

    #[test]
    fn a_prefix_too_long_for_the_engine_is_refused_before_any_ddl() {
        let prefix = format!("{}_", "a".repeat(60));
        let error = ReclaimedDatabase::claim("postgresql://localhost/postgres", &prefix)
            .expect_err("the name would exceed the identifier limit");
        assert!(matches!(error, EphemeralNameError::TooLong { .. }));
    }

    #[test]
    fn a_prefix_that_would_not_be_sweepable_is_refused() {
        let error = ReclaimedDatabase::claim("postgresql://localhost/postgres", "layer5_test")
            .expect_err("a prefix must carry its separator");
        assert!(matches!(
            error,
            EphemeralNameError::PrefixNotSeparated { .. }
        ));
    }
}
