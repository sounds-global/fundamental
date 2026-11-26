use std::rc::Rc;

use fundamental_offline::db::OfflineDatabase;
use fundamental_ws_client::SyncClientConfig;
use futures::future::{LocalBoxFuture, Shared};
use leptos::prelude::{LocalStorage, StoredValue, provide_context};

/// The sync configuration every live hook needs, provided once at the app root.
#[derive(Clone)]
pub struct LiveContext {
    #[cfg_attr(
        not(target_arch = "wasm32"),
        expect(
            dead_code,
            reason = "consumed only in the browser build; the server renders inert"
        )
    )]
    pub(crate) mode: LiveMode,
}

/// Who owns the database this context reads from.
#[cfg_attr(
    not(target_arch = "wasm32"),
    expect(
        dead_code,
        reason = "consumed only in the browser build; the server renders inert"
    )
)]
#[derive(Clone)]
pub(crate) enum LiveMode {
    /// The webview owns the database and its sync sessions.
    Owned {
        config: SyncClientConfig,
        db: StoredValue<LiveDb, LocalStorage>,
    },
    /// A native host owns the database; reads and change events cross the invoke channel.
    Hosted,
}

/// The owned mode's database, opened once and shared by every hook in the lazy arm.
#[derive(Clone)]
pub(crate) enum LiveDb {
    /// The database the app opened itself before providing the context.
    Ready(Rc<OfflineDatabase>),
    /// A one-time open, started by the first hook that awaits it.
    Lazy(Shared<LocalBoxFuture<'static, Option<Rc<OfflineDatabase>>>>),
}

impl LiveDb {
    /// Resolves the database, opening it on first use; `None` when the open failed.
    #[cfg_attr(
        not(target_arch = "wasm32"),
        expect(
            dead_code,
            reason = "consumed only in the browser build; the server renders inert"
        )
    )]
    pub(crate) async fn ready(&self) -> Option<Rc<OfflineDatabase>> {
        match self {
            Self::Ready(db) => Some(Rc::clone(db)),
            Self::Lazy(open) => open.clone().await,
        }
    }
}

/// Provides the app-wide [`LiveContext`] over an already-open database.
pub fn provide_live(config: SyncClientConfig, db: Rc<OfflineDatabase>) {
    provide_context(LiveContext {
        mode: LiveMode::Owned {
            config,
            db: StoredValue::new_local(LiveDb::Ready(db)),
        },
    });
}
