//! An offline record's translations are kept under its own key column, typed as the record's key
//! is: an integer-keyed record's translations are found, deleted and returned by that integer.

#![cfg(not(target_arch = "wasm32"))]

use fundamental_core::i18n::{Locale, Localized};
use fundamental_core::model::OfflineModel;
use fundamental_macros::Model;
use fundamental_offline::db::{OfflineConfig, OfflineDatabase, Value};
use fundamental_offline::store::{OfflineStore, SchemaAction};

/// The native driver's futures are immediately ready.
fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    use std::task::{Context, Poll, Waker};

    let mut cx = Context::from_waker(Waker::noop());
    let mut fut = std::pin::pin!(fut);
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

fn open() -> OfflineDatabase {
    block_on(OfflineDatabase::open(OfflineConfig::memory())).expect("an in-memory database")
}

/// A draft keyed by an integer, with a title in several languages.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(offline)]
struct Draft {
    #[model(id)]
    id: i64,
    title: Localized<String>,
}

fn draft(id: i64, title: &str, dutch: &str) -> Draft {
    Draft {
        id,
        title: Localized::new(title).with("nl", dutch),
    }
}

fn dutch() -> Locale {
    "nl".parse().expect("a language")
}

#[test]
fn an_integer_keyed_record_keeps_its_translations_and_is_found_by_its_integer() {
    let db = open();
    let store = OfflineStore::new(&db);
    store.ensure_schema::<Draft>().expect("the tables");
    store.insert(&draft(7, "News", "Nieuws")).expect("insert");

    let loaded: Draft = store
        .get(&Value::Integer(7))
        .expect("read")
        .expect("the draft");
    assert_eq!(loaded.title.resolve(&dutch()), "Nieuws");
    assert_eq!(
        store
            .find_ids_by_translation::<Draft>("title", "nl", "Nieuws")
            .expect("search"),
        [Value::Integer(7)],
        "the key comes back as the record's own integer, not its text"
    );
    let stored = db
        .query("SELECT typeof(id) FROM draft_i18n;", &[])
        .expect("read the companion");
    assert_eq!(
        stored[0].get(0),
        Some(&Value::Text("integer".to_owned())),
        "the companion stores the key with the record's own type"
    );
}

#[test]
fn a_page_of_records_is_read_with_only_its_own_translations() {
    let db = open();
    let store = OfflineStore::new(&db);
    store.ensure_schema::<Draft>().expect("the tables");
    store.insert(&draft(1, "One", "Een")).expect("insert");
    store.insert(&draft(2, "Two", "Twee")).expect("insert");

    let page: Vec<Draft> = store
        .query_filtered("id = ?1", "", &[Value::Integer(2)])
        .expect("read a page");

    assert_eq!(page.len(), 1);
    assert_eq!(page[0].title.resolve(&dutch()), "Twee");
}

#[test]
fn a_record_given_twice_is_hydrated_both_times() {
    let db = open();
    let store = OfflineStore::new(&db);
    store.ensure_schema::<Draft>().expect("the tables");
    store.insert(&draft(7, "News", "Nieuws")).expect("insert");
    store
        .insert(&draft(8, "Sport", "Sport (nl)"))
        .expect("insert");
    let bare = |id| Draft {
        id,
        title: Localized::new("News"),
    };

    let mut models = vec![bare(7), bare(8), bare(7)];
    store
        .hydrate_translations(&mut models)
        .expect("hydrate the slice");

    let dutch_titles: Vec<String> = models
        .iter()
        .map(|m| m.title.resolve(&dutch()).clone())
        .collect();
    assert_eq!(dutch_titles, ["Nieuws", "Sport (nl)", "Nieuws"]);
}

#[test]
fn a_record_deleted_by_filter_takes_its_translations_with_it() {
    let db = open();
    let store = OfflineStore::new(&db);
    store.ensure_schema::<Draft>().expect("the tables");
    store.insert(&draft(1, "One", "Een")).expect("insert");
    store.insert(&draft(2, "Two", "Twee")).expect("insert");

    let deleted = store
        .delete_filtered::<Draft>("id = ?1", &[Value::Integer(1)])
        .expect("delete by filter");

    assert_eq!(deleted, 1);
    let left = db
        .query("SELECT id FROM draft_i18n;", &[])
        .expect("read the companion");
    assert_eq!(left.len(), 1, "only the kept record's translation is left");
    assert_eq!(left[0].get(0), Some(&Value::Integer(2)));
}

#[test]
fn a_key_deleted_and_written_again_has_none_of_the_old_translations() {
    let db = open();
    let store = OfflineStore::new(&db);
    store.ensure_schema::<Draft>().expect("the tables");
    store.insert(&draft(7, "News", "Nieuws")).expect("insert");
    assert!(store.delete::<Draft>(&Value::Integer(7)).expect("delete"));

    store
        .insert(&Draft {
            id: 7,
            title: Localized::new("Other"),
        })
        .expect("insert again");

    let loaded: Draft = store
        .get(&Value::Integer(7))
        .expect("read")
        .expect("the draft");
    assert_eq!(loaded.title.resolve(&dutch()), "Other");
}

#[test]
fn a_cache_with_a_text_keyed_companion_is_rebuilt_keyed_by_the_record() {
    let db = open();
    db.execute_batch(
        "CREATE TABLE _fundamental_schema (table_name TEXT NOT NULL PRIMARY KEY, \
         schema_version INTEGER NOT NULL);\n\
         CREATE TABLE draft (id INTEGER NOT NULL, title TEXT NOT NULL, PRIMARY KEY (id));\n\
         CREATE TABLE draft_i18n (entity_id TEXT NOT NULL, field TEXT NOT NULL, \
         locale TEXT NOT NULL, value TEXT NOT NULL, source_hash TEXT NOT NULL DEFAULT '', \
         translation_hash TEXT NOT NULL DEFAULT '', checked_by TEXT, checked_at INTEGER, \
         PRIMARY KEY (entity_id, field, locale));",
    )
    .expect("a cache from before");
    let before = 0x1234_i64;
    db.execute(
        "INSERT INTO _fundamental_schema (table_name, schema_version) VALUES ('draft', ?1);",
        &[Value::Integer(before)],
    )
    .expect("its version");
    assert_ne!(<Draft as OfflineModel>::SCHEMA_VERSION, before as u64);

    let store = OfflineStore::new(&db);
    assert_eq!(
        store.ensure_schema::<Draft>().expect("the tables"),
        SchemaAction::Recreated
    );
    store.insert(&draft(7, "News", "Nieuws")).expect("insert");

    let columns = db
        .query("SELECT name FROM pragma_table_info('draft_i18n');", &[])
        .expect("the companion's columns");
    assert_eq!(columns[0].get(0), Some(&Value::Text("id".to_owned())));
}
