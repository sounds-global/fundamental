use fundamental_offline::store::OfflineRecord;
use fundamental_sync::{Slice, SliceBinding, Syncable};
use leptos::prelude::*;
use tracing::error;

#[cfg(target_arch = "wasm32")]
use super::LiveContext;
#[cfg(target_arch = "wasm32")]
use super::live_context::{LiveDb, LiveMode};
#[cfg(target_arch = "wasm32")]
use fundamental_ws_client::SyncClientConfig;

/// Binds a parameter-less slice and returns a [`Signal`] of its rows, kept live in the browser.
///
/// During server rendering the signal is empty and nothing connects.
///
/// # Panics
/// In the browser build, panics if no [`LiveContext`](super::LiveContext) is in
/// scope — call
/// [`provide_live`](super::provide_live) or
/// [`provide_live_lazy`](super::provide_live_lazy) at the app root first.
#[must_use]
pub fn use_live<S>() -> Signal<Vec<S::Model>>
where
    S: Slice,
    S::Model: OfflineRecord + Syncable + Clone + Send + Sync + 'static,
    S::Params: Default,
{
    use_live_with::<S>(S::Params::default())
}

/// Binds a parameterized slice `S` with `params` and returns a [`Signal`] of its rows.
///
/// # Panics
/// In the browser build, panics if no [`LiveContext`](super::LiveContext) is in
/// scope — call
/// [`provide_live`](super::provide_live) or
/// [`provide_live_lazy`](super::provide_live_lazy) at the app root first.
#[must_use]
pub fn use_live_with<S>(params: S::Params) -> Signal<Vec<S::Model>>
where
    S: Slice,
    S::Model: OfflineRecord + Syncable + Clone + Send + Sync + 'static,
{
    #[cfg(target_arch = "wasm32")]
    {
        let context = expect_context::<LiveContext>();

        match SliceBinding::for_slice::<S>(params) {
            Ok(binding) => match context.mode {
                LiveMode::Owned { config, db } => live_signal::<S::Model>(config, db, binding),
                #[cfg(feature = "tauri")]
                LiveMode::Hosted => super::hosted::hosted_signal::<S::Model>(binding),
                #[cfg(not(feature = "tauri"))]
                LiveMode::Hosted => {
                    let _ = binding;
                    error!(
                        slice = S::NAME,
                        "hosted live context outside a native webview; live view is empty"
                    );
                    RwSignal::new(Vec::new()).into()
                }
            },
            Err(error) => {
                error!(%error, slice = S::NAME, "invalid slice parameters; live view is empty");
                RwSignal::new(Vec::new()).into()
            }
        }
    }

    // Server rendering is inert, but the binding is still validated so a bad call site is logged.
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Err(error) = SliceBinding::for_slice::<S>(params) {
            error!(%error, slice = S::NAME, "invalid slice parameters; live view is empty");
        }
        RwSignal::new(Vec::new()).into()
    }
}

/// Starts a sync session for `binding` and returns a signal of `M`'s local rows. Browser only.
#[cfg(target_arch = "wasm32")]
fn live_signal<M>(
    config: SyncClientConfig,
    db: StoredValue<LiveDb, LocalStorage>,
    binding: SliceBinding,
) -> Signal<Vec<M>>
where
    M: OfflineRecord + Syncable + Clone + Send + Sync + 'static,
{
    use std::rc::Rc;

    use fundamental_offline::db::OfflineDatabase;
    use fundamental_offline::store::OfflineStore;
    use fundamental_ws_client::SyncClient;
    use futures::StreamExt;
    use futures::future::abortable;
    use leptos::task::spawn_local;

    let rows = RwSignal::new(Vec::<M>::new());

    let live_db = db.get_value(); // the browser-thread slot (via `GetValue`)

    let work = async move {
        let Some(db) = live_db.ready().await else {
            return;
        };
        // Aborting the task drops the handle, which stops the session.
        let _session = SyncClient::start::<M>(config, binding, Rc::clone(&db));

        let mut changes = db.watch(M::MIRROR_TABLE);
        let refresh = |db: &OfflineDatabase| match OfflineStore::new(db).all::<M>() {
            Ok(items) => Some(items),
            Err(error) => {
                error!(%error, table = M::MIRROR_TABLE, "reading synced rows failed");
                None
            }
        };
        if let Some(items) = refresh(&db) {
            rows.set(items); // seed from whatever is already local
        }
        while changes.next().await.is_some() {
            if let Some(items) = refresh(&db) {
                rows.set(items);
            }
        }
    };

    // `on_cleanup` needs a `Send + Sync` closure, so it holds only the abort handle.
    let (task, abort) = abortable(work);
    on_cleanup(move || abort.abort());
    spawn_local(async move {
        let _ = task.await;
    });

    rows.into()
}
