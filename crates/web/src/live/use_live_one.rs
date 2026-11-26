use fundamental_offline::store::OfflineRecord;
use fundamental_sync::{Slice, Syncable};
use leptos::prelude::*;

use super::use_live;

/// Binds a parameter-less slice and returns a [`Signal`] of its single row, `None` until synced.
///
/// # Panics
/// Panics if no [`LiveContext`](super::LiveContext) is in scope — call
/// [`provide_live`](super::provide_live) at the app root first.
#[must_use]
pub fn use_live_one<S>() -> Signal<Option<S::Model>>
where
    S: Slice,
    S::Model: OfflineRecord + Syncable + Clone + Send + Sync + 'static,
    S::Params: Default,
{
    let rows = use_live::<S>();
    Signal::derive(move || rows.get().into_iter().next())
}
