//! The fake keeps the port's contract, so an app testing against it tests the real shape.

use fundamental_storage::{Conformance, MemoryObjectStore};

#[tokio::test]
async fn the_memory_store_passes_the_conformance_suite() {
    Conformance::check(&MemoryObjectStore::new(), "conformance").await;
}
