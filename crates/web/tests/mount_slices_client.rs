//! `mount_slices!` registers a surface's slices on the server.
#![cfg(all(feature = "ssr", not(target_arch = "wasm32")))]

use fundamental_macros::{mount_slices, slice};
use fundamental_sync::{NoParams, ServerSyncable};
use fundamental_sync_server::SliceRegistry;

// Hand-rolled `ServerSyncable`, so the test needs no offline or model-derive stack.
struct Animal;
impl ServerSyncable for Animal {
    const TABLE: &'static str = "animals";
    const KEY_COLUMNS: &'static [&'static str] = &["id"];
    const PARTITION_COLUMNS: &'static [&'static str] = &["id"];
    const BIDIRECTIONAL: bool = false;
    fn data_columns() -> &'static [&'static str] {
        &["name", "owner_id"]
    }
}

#[slice(Animal, "mine")]
async fn mine(ctx: SliceContext, _p: NoParams) -> Result<Filter, SliceError> {
    Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
}

mount_slices! { Mine }

#[test]
fn mount_slices_registers_the_listed_slice_on_the_server() {
    let registry = register_slices(SliceRegistry::new());
    assert!(registry.contains("mine"));
}
