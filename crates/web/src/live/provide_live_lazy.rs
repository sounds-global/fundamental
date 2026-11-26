use std::rc::Rc;

use fundamental_offline::db::{OfflineConfig, OfflineDatabase};
use fundamental_ws_client::SyncClientConfig;
use futures::FutureExt;
use leptos::prelude::{StoredValue, provide_context};
use tracing::error;

use super::live_context::{LiveContext, LiveDb, LiveMode};

/// Provides the app-wide [`LiveContext`](super::LiveContext) now, opening the database on first use.
///
/// A failed open is logged once and every live view renders empty.
pub fn provide_live_lazy(config: SyncClientConfig, offline: OfflineConfig) {
    let open = async move {
        match OfflineDatabase::open(offline).await {
            Ok(db) => Some(Rc::new(db)),
            Err(error) => {
                // Logged once: the shared future memoizes this `None`.
                error!(%error, "opening the offline database failed; live views stay empty");
                None
            }
        }
    }
    .boxed_local()
    .shared();

    provide_context(LiveContext {
        mode: LiveMode::Owned {
            config,
            db: StoredValue::new_local(LiveDb::Lazy(open)),
        },
    });
}
