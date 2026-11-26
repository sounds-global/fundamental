//! [`sweep_keyspaces_once`] — the keyspace half of [`sweep_once`](super::sweep_once).

use fundamental_cql::CqlConfig;
use fundamental_cql::test_support::drop_stale_ephemeral_keyspaces;
use tokio::sync::OnceCell;
use tracing::{info, warn};

use fundamental_db::ephemeral::stale_cutoff_millis;

static SWEPT: OnceCell<()> = OnceCell::const_new();

/// Removes ephemeral keyspaces old enough that nobody can still be using them.
///
/// CQL cannot tell whether a keyspace has a session attached, so the six-hour age bound stands
/// alone.
pub(super) async fn sweep_keyspaces_once(config: &CqlConfig) {
    SWEPT
        .get_or_init(|| async {
            match drop_stale_ephemeral_keyspaces(config, stale_cutoff_millis()).await {
                Ok(0) => info!("ephemeral: nothing stale to sweep"),
                Ok(reclaimed) => info!(
                    reclaimed,
                    "ephemeral: reclaimed keyspaces left behind by earlier runs"
                ),
                Err(error) => warn!(
                    %error,
                    "ephemeral: could not sweep stale keyspaces; provisioning continues"
                ),
            }
        })
        .await;
}
