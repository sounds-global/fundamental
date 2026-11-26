//! `use_live` renders an inert, empty view during server rendering.

#![cfg(all(feature = "live", not(target_arch = "wasm32")))]

use std::rc::Rc;

use fundamental_core::model::offline::ConflictStrategy;
use fundamental_core::secrets::SecretString;
use fundamental_macros::{Model, slice};
use fundamental_offline::db::{OfflineConfig, OfflineDatabase};
use fundamental_sync::{NoParams, SliceBinding, Syncable};
use fundamental_web::live::{
    host_sync_config, provide_live, provide_live_hosted, provide_live_lazy, use_live,
};
use fundamental_web::prelude::*;
use fundamental_web::render_to_html;
use fundamental_ws_client::SyncClientConfig;

#[derive(Clone, Debug, Model)]
#[model(offline, syncable, table = "widgets")]
struct Widget {
    #[model(id)]
    id: String,
    #[model(owner)]
    owner_id: String,
    name: String,
}

#[slice(Widget, "mine")]
async fn mine(ctx: SliceContext) -> Result<Filter, SliceError> {
    Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
}

#[test]
fn for_slice_derives_the_table_and_strategy_from_the_model() {
    let binding: SliceBinding =
        SliceBinding::for_slice::<Mine>(NoParams::default()).expect("binds");
    // Table and strategy come from the model's `Syncable` metadata, so they cannot disagree.
    assert_eq!(binding.slice(), "mine");
    assert_eq!(binding.local_table(), Widget::MIRROR_TABLE);
    assert!(matches!(
        binding.strategy(),
        ConflictStrategy::LastWriteWins
    ));
}

#[tokio::test]
async fn use_live_is_empty_and_does_not_connect_on_the_server() {
    let db = Rc::new(
        OfflineDatabase::open(OfflineConfig::memory())
            .await
            .expect("open in-memory db"),
    );
    let config = SyncClientConfig::new("ws://127.0.0.1:0", SecretString::new("token".to_string()));

    let html = render_to_html(move || {
        provide_live(config, db);
        let widgets = use_live::<Mine>();
        view! { <ul data-count=move || widgets.get().len()></ul> }
    });

    assert!(html.contains("<ul"), "rendered a list shell: {html}");
    assert!(
        html.contains("data-count=\"0\""),
        "empty on the server: {html}"
    );
}

#[test]
fn host_sync_config_is_none_on_the_server() {
    assert!(host_sync_config().is_none());
}

#[test]
fn use_live_needs_no_context_on_the_server() {
    // A native app's in-process renderer provides no LiveContext, so the server arm needs none.
    let html = render_to_html(move || {
        let widgets = use_live::<Mine>();
        view! { <ul data-count=move || widgets.get().len()></ul> }
    });

    assert!(
        html.contains("data-count=\"0\""),
        "renders empty without a provided context: {html}"
    );
}

#[test]
fn provide_live_hosted_is_inert_on_the_server() {
    let html = render_to_html(move || {
        provide_live_hosted();
        let widgets = use_live::<Mine>();
        view! { <ul data-count=move || widgets.get().len()></ul> }
    });

    assert!(
        html.contains("data-count=\"0\""),
        "empty on the server: {html}"
    );
}

#[test]
fn provide_live_lazy_is_synchronous_and_inert_on_the_server() {
    // The lazy variant must not open the database during server rendering.
    let config = SyncClientConfig::new("ws://127.0.0.1:0", SecretString::new("token".to_string()));

    let html = render_to_html(move || {
        provide_live_lazy(config, OfflineConfig::memory());
        let widgets = use_live::<Mine>();
        view! { <ul data-count=move || widgets.get().len()></ul> }
    });

    assert!(
        html.contains("data-count=\"0\""),
        "empty on the server: {html}"
    );
}
