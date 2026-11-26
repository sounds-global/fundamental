//! `#[slice]` and `mount_slices!` feed `SliceServerSource`, a relational slice included.

use std::collections::BTreeMap;

use fundamental_auth::{AuthenticatedPrincipal, UserId};
use fundamental_macros::{Model, mount_slices, slice};
use fundamental_sync::{MemorySliceStore, SyncMeta, SyncRecord};
use fundamental_sync_server::{NoParams, SliceRegistry, SliceServerSource};
use serde_json::Value;

#[derive(Clone, Debug, Model)]
#[model(offline, syncable, table = "animals")]
struct Animal {
    #[model(id)]
    id: String,
    name: String,
    owner_id: String,
    channel_id: String,
}

#[derive(Clone, Debug, Model)]
#[model(offline, syncable, table = "memberships")]
struct Membership {
    #[model(id)]
    id: String,
    user_id: String,
    channel_id: String,
}

#[slice(Animal, "mine")]
async fn mine(ctx: SliceContext, _params: NoParams) -> Result<Filter, SliceError> {
    Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
}

#[slice(Animal, "in_my_channels")]
async fn in_my_channels(ctx: SliceContext, _params: NoParams) -> Result<Filter, SliceError> {
    let channels = ctx
        .read_column::<Membership>(
            "channel_id",
            &[Filter::eq("user_id", ctx.user_id().as_str())],
        )
        .await?;
    Ok(Filter::in_list("channel_id", channels))
}

mount_slices! { Mine, InMyChannels }

fn principal(user: &str) -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        user_id: UserId::new(user),
        device_id: None,
        scopes: Vec::new(),
        expires_at: 0,
    }
}

fn animal(id: &str, owner: &str, channel: &str) -> SyncRecord {
    let mut payload = BTreeMap::new();
    payload.insert("name".to_string(), format!("animal-{id}"));
    payload.insert("owner_id".to_string(), owner.to_string());
    payload.insert("channel_id".to_string(), channel.to_string());
    SyncRecord {
        id: id.into(),
        meta: SyncMeta::new(1000, 1),
        payload,
    }
}

fn membership(id: &str, user: &str, channel: &str) -> SyncRecord {
    let mut payload = BTreeMap::new();
    payload.insert("user_id".to_string(), user.to_string());
    payload.insert("channel_id".to_string(), channel.to_string());
    SyncRecord {
        id: id.into(),
        meta: SyncMeta::new(1000, 1),
        payload,
    }
}

fn seeded_source() -> SliceServerSource<MemorySliceStore> {
    let store = MemorySliceStore::new();
    store.put("animals", animal("a-1", "alice", "c-1"));
    store.put("animals", animal("a-2", "alice", "c-2"));
    store.put("animals", animal("a-3", "bob", "c-1"));
    store.put("memberships", membership("m-1", "alice", "c-1"));

    let registry = register_slices(SliceRegistry::new());
    SliceServerSource::new(registry, store)
}

#[tokio::test]
async fn principal_scoped_slice_pulls_only_the_principals_rows() {
    let source = seeded_source();
    let (records, _next) = source
        .pull(&principal("alice"), "mine", Value::Null, 0, None, 100)
        .await
        .expect("mine pulls");

    let mut ids: Vec<String> = records.iter().map(|r| r.id.to_string()).collect();
    ids.sort_unstable();
    assert_eq!(ids, vec!["a-1", "a-2"], "alice's animals only, not bob's");
}

#[tokio::test]
async fn relational_slice_reads_a_second_model_to_scope_the_pull() {
    let source = seeded_source();
    let (records, _next) = source
        .pull(
            &principal("alice"),
            "in_my_channels",
            Value::Null,
            0,
            None,
            100,
        )
        .await
        .expect("relational slice pulls");

    let mut ids: Vec<String> = records.iter().map(|r| r.id.to_string()).collect();
    ids.sort_unstable();
    assert_eq!(ids, vec!["a-1", "a-3"]);
}

#[tokio::test]
async fn registering_the_slices_makes_them_resolvable_by_name() {
    let source = seeded_source();
    let err = source
        .pull(&principal("alice"), "ghost", Value::Null, 0, None, 100)
        .await
        .expect_err("unmounted slice is unknown");
    assert!(err.to_string().contains("Unknown slice"), "{err}");
    assert!(
        !err.to_string().contains("ghost"),
        "the client's name is not quoted back: {err}"
    );
}
