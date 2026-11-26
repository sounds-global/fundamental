//! [`OfflineMirror`] as the engine's [`LocalMirror`], over an in-memory SQLite database.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use fundamental_core::model::offline::ConflictStrategy;
use fundamental_db::RecordKey;
use fundamental_macros::Model;
use fundamental_offline::db::{OfflineConfig, OfflineDatabase};
use fundamental_sync::adapters::OfflineMirror;
use fundamental_sync::engine::{RemoteSource, SliceBinding, SyncEngine, SyncMeta, SyncRecord};
use fundamental_sync::{SyncError, Syncable};
use serde_json::Value;

/// A binding over the `notes` slice into `Note`'s mirror table.
fn binding() -> SliceBinding {
    SliceBinding::new(
        "notes",
        Note::MIRROR_TABLE,
        Value::Null,
        ConflictStrategy::LastWriteWins,
    )
}

#[derive(Clone, Debug, Model)]
#[model(offline, syncable)]
struct Note {
    #[model(id)]
    id: String,
    body: String,
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

async fn open_with_mirror() -> OfflineDatabase {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("in-memory db");
    db.execute_batch(
        "CREATE TABLE note (\
            id TEXT NOT NULL PRIMARY KEY,\
            body TEXT,\
            updated_at INTEGER NOT NULL,\
            version INTEGER NOT NULL,\
            _tombstone INTEGER NOT NULL)",
    )
    .expect("create mirror table");
    db
}

#[tokio::test]
async fn pull_persists_remote_rows_into_the_mirror_table() {
    let db = open_with_mirror().await;
    let mirror = OfflineMirror::<Note>::new(&db).expect("mirror");
    let remote = FakeRemote::default();
    remote.seed(record("a", 10, "alpha"));

    let outcome = SyncEngine
        .pull(&mirror, &remote, &binding())
        .await
        .expect("pull");
    assert_eq!(outcome.applied, 1);

    let rows = db
        .query("SELECT body FROM note WHERE id = ?1", &[("a").into()])
        .expect("query");
    assert_eq!(rows[0].get_named("body").unwrap().as_text(), Some("alpha"));
}

#[tokio::test]
async fn local_change_is_pushed_and_then_clean() {
    let db = open_with_mirror().await;
    let mirror = OfflineMirror::<Note>::new(&db).expect("mirror");
    let remote = FakeRemote::default();

    mirror
        .record_local_change(&record("b", 99, "local-edit"))
        .expect("record local change");

    let pushed = SyncEngine
        .push(&mirror, &remote, &binding())
        .await
        .expect("push");
    assert_eq!(pushed, 1);
    assert_eq!(remote.get("b").unwrap().payload["body"], "local-edit");

    let again = SyncEngine
        .push(&mirror, &remote, &binding())
        .await
        .expect("second push");
    assert_eq!(again, 0);
}
