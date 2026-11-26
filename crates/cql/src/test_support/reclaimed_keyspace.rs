//! [`ReclaimedKeyspace`] — a per-test keyspace name that owns its own reclamation.

use fundamental_db::ephemeral::{EphemeralName, EphemeralNameError};

use super::drop_test_keyspace;
use crate::CqlConfig;

/// A keyspace name's limit — the tighter of the two engines'.
const MAX_KEYSPACE_NAME: usize = 48;

/// A minted keyspace name whose reclamation is already guaranteed when it is handed out.
///
/// Minted before any DDL, so a failure or panic between create and guard still reclaims.
///
/// # Example
///
/// ```no_run
/// # use fundamental_cql::CqlConfig;
/// # use fundamental_cql::test_support::ReclaimedKeyspace;
/// # async fn provision(config: CqlConfig) -> Result<(), Box<dyn std::error::Error>> {
/// // Nothing exists on the node yet, and the name is already accounted for.
/// let reclaimed = ReclaimedKeyspace::claim(config, "vault_test_")?;
///
/// // Every failure from here on reclaims, including a panic.
/// create_keyspace(reclaimed.name()).await?;
/// migrate(reclaimed.name()).await?;
/// # Ok(())
/// # }
/// # async fn create_keyspace(_: &str) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
/// # async fn migrate(_: &str) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
/// ```
#[derive(Debug)]
pub struct ReclaimedKeyspace {
    name: EphemeralName,
    /// How teardown reconnects: the settings, never a live pool, and the whole config so a drop
    /// reaches the node as the create did.
    config: CqlConfig,
}

impl ReclaimedKeyspace {
    /// Mints a name and takes ownership of removing whatever is created under it.
    ///
    /// `prefix` includes its trailing separator.
    ///
    /// # Errors
    /// Returns [`EphemeralNameError`] if the prefix is not a plain identifier, does not
    /// end with `_`, or would mint a name longer than a keyspace allows.
    pub fn claim(config: CqlConfig, prefix: &str) -> Result<Self, EphemeralNameError> {
        Ok(Self {
            name: EphemeralName::mint(prefix, MAX_KEYSPACE_NAME)?,
            config,
        })
    }

    /// The keyspace name, for the DDL that is about to create it.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// The connection the create and the drop both run from.
    #[must_use]
    pub fn config(&self) -> &CqlConfig {
        &self.config
    }
}

impl Drop for ReclaimedKeyspace {
    fn drop(&mut self) {
        drop_test_keyspace(&self.config, self.name.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_node_config;

    fn node() -> CqlConfig {
        test_node_config(["127.0.0.1:9042"])
    }

    #[test]
    fn a_claim_names_a_keyspace_before_one_exists() {
        let reclaimed =
            ReclaimedKeyspace::claim(node(), "vault_test_").expect("a plain prefix is accepted");
        assert!(reclaimed.name().starts_with("vault_test_"));
        assert!(
            EphemeralName::stamp_of(reclaimed.name()).is_some(),
            "{} must be sweepable",
            reclaimed.name()
        );
        assert!(reclaimed.name().len() <= MAX_KEYSPACE_NAME);
    }

    #[test]
    fn a_prefix_too_long_for_a_keyspace_is_refused_before_any_ddl() {
        // A readable prefix plus the stamp and tail is what overflows, and it must fail here.
        let error = ReclaimedKeyspace::claim(node(), "a_very_long_application_test_")
            .expect_err("the name would exceed the keyspace limit");
        assert!(matches!(error, EphemeralNameError::TooLong { .. }));
    }

    #[test]
    fn a_prefix_that_would_not_be_sweepable_is_refused() {
        let error = ReclaimedKeyspace::claim(node(), "vault_test")
            .expect_err("a prefix must carry its separator");
        assert!(matches!(
            error,
            EphemeralNameError::PrefixNotSeparated { .. }
        ));
    }
}
