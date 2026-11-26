//! The typed device-slice path end to end, rows round-tripping to typed models.

#![cfg(feature = "sync-host")]

use std::rc::Rc;

use fundamental_core::Error as _;
use fundamental_core::model::OfflineModel;
use fundamental_macros::{Model, mount_device_slices, slice};
use fundamental_native::{SyncHost, SyncHostConfig};
use fundamental_offline::db::{OfflineConfig, OfflineDatabase, OfflineLocation, Value};
use fundamental_sync::{NoParams, Syncable};
use serde_json::json;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(offline, syncable, table = "widgets")]
struct Widget {
    #[model(id)]
    id: String,
    #[model(owner)]
    owner_id: String,
    name: String,
}

#[slice(Widget, "mine")]
async fn mine(ctx: SliceContext, _p: NoParams) -> Result<Filter, SliceError> {
    Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
}

mount_device_slices! { Mine }

fn host_config() -> SyncHostConfig {
    SyncHostConfig::new("ws://127.0.0.1:0")
}

#[test]
fn mount_device_slices_registers_the_listed_slice() {
    let registry = device_slices();
    assert_eq!(registry.len(), 1);
    let entry = registry.get("mine").expect("listed slice resolves");
    assert_eq!(entry.slice(), "mine");
    assert_eq!(entry.table(), Widget::MIRROR_TABLE);
    assert!(registry.get("other").is_err());
}

#[test]
fn a_parameter_less_slice_is_eager_and_validates_its_params() {
    let registry = device_slices();
    let entry = registry.get("mine").expect("resolves");

    // NoParams accepts the empty object, so the slice syncs eagerly.
    let eager = entry.eager_binding().expect("NoParams slice is eager");
    assert_eq!(eager.slice(), "mine");
    assert_eq!(eager.local_table(), Widget::MIRROR_TABLE);

    // Non-object params cannot deserialize into NoParams: a typed 422.
    let err = entry.binding(json!(42)).expect_err("bad params");
    assert_eq!(err.error_code(), "NATIVE_018");
    assert_eq!(err.http_status_code(), 422);
}

#[tokio::test]
async fn rows_round_trip_from_mirror_to_typed_models() {
    let registry = device_slices();
    let entry = registry.get("mine").expect("resolves");

    let db = Rc::new(
        OfflineDatabase::open(OfflineConfig::memory())
            .await
            .expect("open in-memory db"),
    );
    entry.ensure_table(&db).expect("mirror table");

    let widget = Widget {
        id: "w1".to_owned(),
        owner_id: "u1".to_owned(),
        name: "hé llo".to_owned(),
    };
    // Seed one row as an applied remote change would land, overlay included.
    db.execute(
        "INSERT INTO widget (id, owner_id, name, updated_at, version, _tombstone) \
         VALUES (?, ?, ?, 0, 1, 0)",
        &[
            Value::Text(widget.id.clone()),
            Value::Text(widget.owner_id.clone()),
            Value::Text(widget.name.clone()),
        ],
    )
    .expect("seed mirror row");

    let rows = entry.rows(&db).expect("read rows");
    let typed: Vec<Widget> = rows.typed().expect("typed round-trip");
    assert_eq!(typed, vec![widget]);
}

#[tokio::test]
async fn the_host_serves_the_registry_end_to_end() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let offline = OfflineConfig {
        location: OfflineLocation::Persistent(
            dir.path().join("sync.db").to_string_lossy().into_owned(),
        ),
        ..OfflineConfig::memory()
    };
    let host =
        SyncHost::start(offline, host_config(), device_slices(), |_| {}).expect("host starts");

    host.bind("mine", json!({})).await.expect("bind");
    let rows = host.rows("mine").await.expect("rows");
    assert_eq!(rows.columns, Widget::column_names());
    assert!(rows.rows.is_empty());
    host.unbind("mine", json!({})).await.expect("unbind");

    let err = host.rows("other").await.expect_err("unknown");
    assert_eq!(err.error_code(), "NATIVE_016");
}
