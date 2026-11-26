//! Files on one host keep the port's contract.

use fundamental_storage::{Conformance, FsObjectStore};

#[tokio::test]
async fn the_file_store_passes_the_conformance_suite() {
    let dir = tempfile::tempdir().expect("a temporary root");
    let store = FsObjectStore::open(dir.path()).await.expect("a store");
    Conformance::check(&store, "conformance").await;
    let staging = std::fs::read_dir(dir.path().join(".staging")).expect("staging");
    assert_eq!(
        staging.count(),
        0,
        "an overrun or failed put leaves no staging file"
    );
}
