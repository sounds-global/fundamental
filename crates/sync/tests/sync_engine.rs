//! [`SyncEngine`] against in-memory fakes.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Mutex;

use fundamental_core::model::offline::ConflictStrategy;
use fundamental_db::RecordKey;
use fundamental_sync::SyncError;
use fundamental_sync::engine::{
    LocalMirror, RemoteSource, SliceBinding, SyncEngine, SyncMeta, SyncRecord,
};
use serde_json::Value;

const TABLE: &str = "notes";

/// A binding over the `mine` slice into the local mirror table, reconciling with `strategy`.
fn binding(strategy: ConflictStrategy) -> SliceBinding {
    SliceBinding::new("mine", TABLE, Value::Null, strategy)
}

fn record(id: &str, updated_at: i64, body: &str) -> SyncRecord {
    let mut payload = BTreeMap::new();
    payload.insert("body".to_string(), body.to_string());
    SyncRecord::new(id, SyncMeta::new(updated_at, 1), payload)
}

#[derive(Default)]
struct FakeRemote {
    rows: Mutex<HashMap<RecordKey, SyncRecord>>,
}

impl FakeRemote {
    fn seed(&self, record: SyncRecord) {
        self.rows.lock().unwrap().insert(record.id.clone(), record);
    }
    fn get(&self, id: &str) -> Option<SyncRecord> {
        self.rows
            .lock()
            .unwrap()
            .get(&RecordKey::single(id))
            .cloned()
    }
}

impl RemoteSource for FakeRemote {
    async fn changed_since(
        &self,
        _slice: &str,
        _params: &Value,
        since: i64,
    ) -> Result<Vec<SyncRecord>, SyncError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .values()
            .filter(|r| r.meta.updated_at > since)
            .cloned()
            .collect())
    }

    async fn upsert(
        &self,
        _slice: &str,
        _params: &Value,
        record: &SyncRecord,
    ) -> Result<(), SyncError> {
        self.rows
            .lock()
            .unwrap()
            .insert(record.id.clone(), record.clone());
        Ok(())
    }
}

#[derive(Default)]
struct FakeLocal {
    rows: Mutex<HashMap<RecordKey, (SyncRecord, bool)>>,
    high_water: Mutex<i64>,
}

impl FakeLocal {
    fn seed_dirty(&self, record: SyncRecord) {
        self.rows
            .lock()
            .unwrap()
            .insert(record.id.clone(), (record, true));
    }
    fn get(&self, id: &str) -> Option<SyncRecord> {
        self.rows
            .lock()
            .unwrap()
            .get(&RecordKey::single(id))
            .map(|(r, _)| r.clone())
    }
}

impl LocalMirror for FakeLocal {
    async fn get(&self, _table: &str, id: &RecordKey) -> Result<Option<SyncRecord>, SyncError> {
        Ok(self.rows.lock().unwrap().get(id).map(|(r, _)| r.clone()))
    }

    async fn apply_remote(&self, _table: &str, record: &SyncRecord) -> Result<(), SyncError> {
        self.rows
            .lock()
            .unwrap()
            .insert(record.id.clone(), (record.clone(), false));
        Ok(())
    }

    async fn dirty(&self, _table: &str) -> Result<Vec<SyncRecord>, SyncError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .values()
            .filter(|(_, dirty)| *dirty)
            .map(|(r, _)| r.clone())
            .collect())
    }

    async fn mark_clean(&self, _table: &str, id: &RecordKey) -> Result<(), SyncError> {
        if let Some((_, dirty)) = self.rows.lock().unwrap().get_mut(id) {
            *dirty = false;
        }
        Ok(())
    }

    async fn last_pulled(&self, _table: &str) -> Result<i64, SyncError> {
        Ok(*self.high_water.lock().unwrap())
    }

    async fn set_last_pulled(&self, _table: &str, updated_at: i64) -> Result<(), SyncError> {
        *self.high_water.lock().unwrap() = updated_at;
        Ok(())
    }
}

#[tokio::test]
async fn pull_applies_new_rows_and_advances_high_water() {
    let local = FakeLocal::default();
    let remote = FakeRemote::default();
    remote.seed(record("a", 10, "alpha"));
    remote.seed(record("b", 20, "beta"));

    let outcome = SyncEngine
        .pull(&local, &remote, &binding(ConflictStrategy::LastWriteWins))
        .await
        .expect("pull");
    assert_eq!(outcome.applied, 2);
    assert_eq!(local.get("a").unwrap().payload["body"], "alpha");

    let again = SyncEngine
        .pull(&local, &remote, &binding(ConflictStrategy::LastWriteWins))
        .await
        .expect("second pull");
    assert_eq!(again.applied, 0);
}

#[tokio::test]
async fn last_write_wins_prefers_the_newer_side() {
    let local = FakeLocal::default();
    let remote = FakeRemote::default();

    local.seed_dirty(record("x", 100, "local-new"));
    remote.seed(record("x", 50, "remote-old"));
    let outcome = SyncEngine
        .pull(&local, &remote, &binding(ConflictStrategy::LastWriteWins))
        .await
        .expect("pull");
    assert_eq!(outcome.kept_local, 1);
    assert_eq!(local.get("x").unwrap().payload["body"], "local-new");

    remote.seed(record("x", 200, "remote-new"));
    let outcome = SyncEngine
        .pull(&local, &remote, &binding(ConflictStrategy::LastWriteWins))
        .await
        .expect("pull");
    assert_eq!(outcome.applied, 1);
    assert_eq!(local.get("x").unwrap().payload["body"], "remote-new");
}

#[tokio::test]
async fn server_wins_overrides_local() {
    let local = FakeLocal::default();
    let remote = FakeRemote::default();
    local.seed_dirty(record("x", 100, "local"));
    remote.seed(record("x", 50, "server"));

    let outcome = SyncEngine
        .pull(&local, &remote, &binding(ConflictStrategy::ServerWins))
        .await
        .expect("pull");
    assert_eq!(outcome.applied, 1);
    assert_eq!(local.get("x").unwrap().payload["body"], "server");
}

#[tokio::test]
async fn push_sends_dirty_rows_and_marks_them_clean() {
    let local = FakeLocal::default();
    let remote = FakeRemote::default();
    local.seed_dirty(record("a", 10, "alpha"));
    local.seed_dirty(record("b", 20, "beta"));

    let pushed = SyncEngine
        .push(&local, &remote, &binding(ConflictStrategy::LastWriteWins))
        .await
        .expect("push");
    assert_eq!(pushed, 2);
    assert_eq!(remote.get("a").unwrap().payload["body"], "alpha");

    let again = SyncEngine
        .push(&local, &remote, &binding(ConflictStrategy::LastWriteWins))
        .await
        .expect("push");
    assert_eq!(again, 0);
}

#[tokio::test]
async fn a_remote_tombstone_propagates_a_delete() {
    let local = FakeLocal::default();
    let remote = FakeRemote::default();
    local.seed_dirty(record("gone", 10, "here"));

    let tombstone = SyncRecord::new("gone", SyncMeta::new(99, 2).tombstoned(), BTreeMap::new());
    remote.seed(tombstone);

    let outcome = SyncEngine
        .pull(&local, &remote, &binding(ConflictStrategy::LastWriteWins))
        .await
        .expect("pull");
    assert_eq!(outcome.applied, 1);
    assert!(
        local.get("gone").unwrap().meta.tombstone,
        "the local mirror row is now a tombstone"
    );
}

#[tokio::test]
async fn custom_strategy_surfaces_conflicts() {
    let local = FakeLocal::default();
    let remote = FakeRemote::default();
    local.seed_dirty(record("x", 100, "local"));
    remote.seed(record("x", 50, "remote"));

    let outcome = SyncEngine
        .pull(&local, &remote, &binding(ConflictStrategy::Custom))
        .await
        .expect("pull");
    assert_eq!(outcome.conflicts, 1);
    assert_eq!(outcome.applied, 0);
    assert_eq!(local.get("x").unwrap().payload["body"], "local");
}
