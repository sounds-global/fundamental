//! A record keyed `(organisation_id, id)` keeps its `Localized` text's other languages through every transactional operation; requires Docker.

use std::sync::{Arc, Mutex};

use fundamental_core::i18n::{Locale, Localized};
use fundamental_core::model::SqlDb;
use fundamental_db::migration::{ModelSchema, render};
use fundamental_db::{
    DatabaseError, IsolationLevel, OrderBy, QueryBuilder, RepositoryTransaction as _,
    RepositoryTransactionQuery as _,
};
use fundamental_macros::Model;
use fundamental_sql::{SqlModelQuery, SqlQuery};
use fundamental_test_containers::sql::SqlSchema;
use tracing_subscriber::layer::{Context, SubscriberExt as _};
use tracing_subscriber::util::SubscriberInitExt as _;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql, table = "notices", soft_delete)]
struct Notice {
    #[model(id, shard = "hash")]
    organisation_id: Uuid,
    #[model(id, shard = "asc")]
    id: Uuid,
    title: Localized<String>,
    position: i32,
}

fn nl() -> Locale {
    "nl".parse().expect("a language")
}

fn notice(organisation_id: Uuid, id: Uuid, english: &str, dutch: &str) -> Notice {
    Notice {
        organisation_id,
        id,
        title: Localized::new(english).with("nl", dutch),
        position: 1,
    }
}

async fn notices() -> SqlSchema {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let own = <Notice as ModelSchema<SqlDb>>::table_schema();
    let companion =
        <Notice as ModelSchema<SqlDb>>::translation_schema().expect("a localized record");
    for statement in render::create_all(&own)
        .into_iter()
        .chain(render::create_all(&companion))
    {
        schema
            .pool()
            .execute_ddl(&statement)
            .await
            .expect("the tables exist");
    }
    schema
}

#[tokio::test]
async fn a_record_written_in_a_transaction_reads_back_with_its_translations() {
    let schema = notices().await;
    let (organisation, id) = (Uuid::new_v4(), Uuid::new_v4());
    let written = notice(
        organisation,
        id,
        "The shop is open",
        "The shop is open voor iedereen",
    );

    let (stored, found, locked) = schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            let stored = tx.insert(&written).await?;
            let found = tx.find_by_key::<Notice>(&(organisation, id)).await?;
            let locked = tx
                .find_for_update_by_key::<Notice>(&(organisation, id))
                .await?;
            Ok::<_, DatabaseError>((stored, found, locked))
        })
        .await
        .expect("written and read");

    assert_eq!(stored, written, "the write answers with what it stored");
    assert_eq!(found.as_ref(), Some(&written));
    assert_eq!(locked.as_ref(), Some(&written));
    assert_eq!(
        found.expect("found").title.resolve(&nl()),
        "The shop is open voor iedereen"
    );
    let key = (organisation, id);
    assert_eq!(
        SqlModelQuery::<Notice>::find_by_key(schema.pool(), &key)
            .await
            .expect("read outside a transaction"),
        Some(written),
        "a read by the whole key outside a transaction carries the translations too"
    );
}

async fn companion_rows(schema: &SqlSchema, organisation: Uuid, id: Uuid) -> Vec<(String, String)> {
    schema
        .pool()
        .fetch_all(&format!(
            "SELECT locale, value FROM notices_i18n WHERE organisation_id = '{organisation}' \
             AND id = '{id}' ORDER BY locale"
        ))
        .await
        .expect("the companion rows")
        .iter()
        .map(|row| {
            (
                fundamental_sql::sqlx::Row::get(row, "locale"),
                fundamental_sql::sqlx::Row::get(row, "value"),
            )
        })
        .collect()
}

#[tokio::test]
async fn an_update_or_an_upsert_leaves_the_companion_holding_what_the_record_holds() {
    let schema = notices().await;
    let (organisation, id) = (Uuid::new_v4(), Uuid::new_v4());
    let mut record = notice(organisation, id, "Open", "Open");
    record.title.set("de", "Offen");
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.insert(&record).await
        })
        .await
        .expect("written");

    let edited = notice(organisation, id, "Open to all", "Open voor iedereen");
    let (updated, read) = schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            let updated = tx.update(&edited).await?;
            let read = tx.find_by_key::<Notice>(&(organisation, id)).await?;
            Ok::<_, DatabaseError>((updated, read))
        })
        .await
        .expect("updated");
    assert_eq!(updated, edited);
    assert_eq!(read, Some(edited));
    assert_eq!(
        companion_rows(&schema, organisation, id).await,
        [("nl".to_owned(), "Open voor iedereen".to_owned())],
        "the language the edit dropped is gone"
    );

    let mut upserted = notice(organisation, id, "Closed", "Gesloten");
    upserted.title.set("fr", "Fermé");
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.upsert(&upserted).await
        })
        .await
        .expect("upserted");
    assert_eq!(
        companion_rows(&schema, organisation, id).await,
        [
            ("fr".to_owned(), "Fermé".to_owned()),
            ("nl".to_owned(), "Gesloten".to_owned())
        ]
    );
}

#[tokio::test]
async fn a_create_that_finds_the_record_there_writes_no_translation() {
    let schema = notices().await;
    let (organisation, id) = (Uuid::new_v4(), Uuid::new_v4());
    let first = notice(organisation, id, "Open", "Open");
    let second = notice(organisation, id, "Again", "Opnieuw");

    let (created, again) = schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            let created = tx.insert_if_absent(&first).await?;
            let again = tx.insert_if_absent(&second).await?;
            Ok::<_, DatabaseError>((created, again))
        })
        .await
        .expect("created once");

    assert_eq!(created, Some(first));
    assert_eq!(again, None);
    assert_eq!(
        companion_rows(&schema, organisation, id).await,
        [("nl".to_owned(), "Open".to_owned())]
    );
}

/// Every statement the driver runs while installed on this thread.
#[derive(Clone, Default)]
struct Statements(Arc<Mutex<Vec<String>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Statements {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        if event.metadata().target() != "sqlx::query" {
            return;
        }
        let mut text = String::new();
        event.record(
            &mut |_: &tracing::field::Field, value: &dyn std::fmt::Debug| {
                text.push_str(&format!("{value:?} "));
            },
        );
        self.0.lock().expect("the statements").push(text);
    }
}

impl Statements {
    fn run(&self) -> usize {
        let mut seen = self.0.lock().expect("the statements");
        let count = seen.len();
        seen.clear();
        count
    }

    fn companion_reads(&self) -> usize {
        let mut seen = self.0.lock().expect("the statements");
        let count = seen
            .iter()
            .filter(|text| text.contains("notices_i18n"))
            .count();
        seen.clear();
        count
    }
}

async fn three_notices(schema: &SqlSchema, organisation: Uuid) -> Vec<Notice> {
    let written: Vec<Notice> = (1..=3)
        .map(|at| Notice {
            position: at,
            ..notice(
                organisation,
                Uuid::new_v4(),
                &format!("Notice {at}"),
                &format!("Bericht {at}"),
            )
        })
        .collect();
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            for row in &written {
                tx.insert(row).await?;
            }
            Ok::<_, DatabaseError>(())
        })
        .await
        .expect("written");
    written
}

#[tokio::test]
async fn a_page_of_records_reads_its_translations_in_one_statement() {
    let schema = notices().await;
    let organisation = Uuid::new_v4();
    let written = three_notices(&schema, organisation).await;

    let statements = Statements::default();
    let _installed = tracing_subscriber::registry()
        .with(statements.clone())
        .set_default();
    let by_position = || {
        QueryBuilder::<Notice, SqlDb>::new()
            .pin_key(organisation)
            .order_by(OrderBy::asc("position"))
    };
    let (filtered, locked, page) = schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            statements.companion_reads();
            let filtered = tx.query_where(&by_position()).await?;
            let filtered_reads = statements.companion_reads();
            let locked = tx
                .query_locked(&SqlQuery::new(by_position()).for_update())
                .await?;
            let locked_reads = statements.companion_reads();
            let page = SqlModelQuery::<Notice>::new(&mut *tx)
                .pin_key(organisation)
                .order_by(OrderBy::asc("position"))
                .page_size(10)
                .execute_page(None)
                .await?;
            let page_reads = statements.companion_reads();
            Ok::<_, DatabaseError>((
                (filtered, filtered_reads),
                (locked, locked_reads),
                (page.items, page_reads),
            ))
        })
        .await
        .expect("read");

    for (rows, reads) in [filtered, locked, page] {
        assert_eq!(rows, written, "every row carries its own translations");
        assert_eq!(reads, 1, "a page reads its translations in one statement");
    }
}

#[tokio::test]
async fn another_organisations_record_of_the_same_id_keeps_its_own_translations() {
    let schema = notices().await;
    let (first, second, id) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let ours = notice(first, id, "Open", "Open bij ons");
    let theirs = notice(second, id, "Open", "Open bij hen");
    let (read_ours, read_theirs) = schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.insert(&ours).await?;
            tx.insert(&theirs).await?;
            let read_ours = tx.find_by_key::<Notice>(&(first, id)).await?;
            let read_theirs = tx.find_by_key::<Notice>(&(second, id)).await?;
            Ok::<_, DatabaseError>((read_ours, read_theirs))
        })
        .await
        .expect("written and read");
    assert_eq!(read_ours, Some(ours));
    assert_eq!(read_theirs, Some(theirs));

    let deleted = schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.delete_where(
                &QueryBuilder::<Notice, SqlDb>::new()
                    .pin_key(first)
                    .filter(fundamental_db::Filter::eq("id", id)),
            )
            .await
        })
        .await
        .expect("deleted");
    assert_eq!(deleted, 1);
    assert!(
        companion_rows(&schema, first, id).await.is_empty(),
        "a deleted record's translations go with it"
    );
    assert_eq!(
        companion_rows(&schema, second, id).await,
        [("nl".to_owned(), "Open bij hen".to_owned())],
        "and never another organisation's"
    );

    let key = (second, id);
    let removed = SqlModelQuery::<Notice>::delete_by_key(schema.pool(), &key)
        .await
        .expect("deleted by its key");
    assert_eq!(removed, 1);
    assert!(companion_rows(&schema, second, id).await.is_empty());
}

#[tokio::test]
async fn a_record_deleted_and_restored_comes_back_with_its_translations() {
    let schema = notices().await;
    let (organisation, id) = (Uuid::new_v4(), Uuid::new_v4());
    let record = notice(organisation, id, "Open", "Open voor iedereen");
    let key = (organisation, id);
    let (gone, restored) = schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.insert(&record).await?;
            tx.soft_delete_by_key::<Notice>(&key).await?;
            let gone = tx.find_by_key::<Notice>(&key).await?;
            tx.restore_by_key::<Notice>(&key).await?;
            let restored = tx.find_for_update_by_key::<Notice>(&key).await?;
            Ok::<_, DatabaseError>((gone, restored))
        })
        .await
        .expect("deleted and restored");
    assert_eq!(gone, None);
    assert_eq!(restored, Some(record));
}

#[tokio::test]
async fn a_record_written_and_read_through_the_pools_repository_keeps_its_translations() {
    let schema = notices().await;
    let (organisation, id) = (Uuid::new_v4(), Uuid::new_v4());
    let written = notice(organisation, id, "Open", "Open voor iedereen");
    let repository = schema.pool().repository::<Notice>();

    let stored = fundamental_db::Repository::insert(&repository, &written)
        .await
        .expect("written");
    let key = (organisation, id);
    let read = repository.find_by_key(&key).await.expect("read");
    let all = repository.find_all(10, 0).await.expect("read them all");

    assert_eq!(stored, written);
    assert_eq!(read.as_ref(), Some(&written));
    assert_eq!(all, [written]);
}

#[tokio::test]
async fn a_page_read_through_the_pools_query_executor_carries_its_translations() {
    let schema = notices().await;
    let organisation = Uuid::new_v4();
    let written = three_notices(&schema, organisation).await;
    let executor = schema.pool().query_executor::<Notice>();
    let by_position = QueryBuilder::<Notice, SqlDb>::new()
        .pin_key(organisation)
        .order_by(OrderBy::asc("position"));

    let page = fundamental_db::QueryExecutor::execute(&executor, &by_position)
        .await
        .expect("read");
    let first = fundamental_db::QueryExecutor::execute_one(&executor, &by_position)
        .await
        .expect("read one");

    assert_eq!(page, written);
    assert_eq!(first.as_ref(), written.first());
}

#[tokio::test]
async fn a_record_deleted_through_the_pools_query_executor_takes_its_translations_with_it() {
    let schema = notices().await;
    let (organisation, id) = (Uuid::new_v4(), Uuid::new_v4());
    let record = notice(organisation, id, "Open", "Open voor iedereen");
    let repository = schema.pool().repository::<Notice>();
    fundamental_db::Repository::insert(&repository, &record)
        .await
        .expect("written");

    let deleted = fundamental_db::QueryExecutor::delete_where(
        &schema.pool().query_executor::<Notice>(),
        &QueryBuilder::<Notice, SqlDb>::new()
            .pin_key(organisation)
            .filter(fundamental_db::Filter::eq("id", id)),
    )
    .await
    .expect("deleted");

    assert_eq!(deleted, 1);
    assert!(
        companion_rows(&schema, organisation, id).await.is_empty(),
        "a deleted record's translations go with it"
    );
    let again = fundamental_db::Repository::insert(&repository, &record)
        .await
        .expect("the same key is written again");
    assert_eq!(again, record);
}

#[tokio::test]
async fn a_write_that_holds_no_other_language_clears_the_companion() {
    let schema = notices().await;
    let (organisation, id) = (Uuid::new_v4(), Uuid::new_v4());
    let mut record = notice(organisation, id, "Open", "Open");
    record.title.set("de", "Offen");
    let english_only = Notice {
        title: Localized::new("Open to all"),
        ..record.clone()
    };
    let (updated, read) = schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.insert(&record).await?;
            let updated = tx.update(&english_only).await?;
            let read = tx.find_by_key::<Notice>(&(organisation, id)).await?;
            Ok::<_, DatabaseError>((updated, read))
        })
        .await
        .expect("written and updated");
    assert_eq!(updated, english_only);
    assert_eq!(read, Some(english_only.clone()));
    assert!(companion_rows(&schema, organisation, id).await.is_empty());

    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.upsert(&record).await?;
            tx.upsert(&english_only).await
        })
        .await
        .expect("upserted");
    assert!(
        companion_rows(&schema, organisation, id).await.is_empty(),
        "an upsert holding the source language alone clears the rest too"
    );
}

#[tokio::test]
async fn the_pools_repository_rewrites_and_removes_a_records_translations() {
    let schema = notices().await;
    let (organisation, id) = (Uuid::new_v4(), Uuid::new_v4());
    let repository = schema.pool().repository::<Notice>();
    let key = (organisation, id);
    let first = notice(organisation, id, "Open", "Open");

    let created = repository
        .insert_or_ignore(&first, &["organisation_id", "id"])
        .await
        .expect("created");
    let again = repository
        .insert_or_ignore(
            &notice(organisation, id, "Again", "Opnieuw"),
            &["organisation_id", "id"],
        )
        .await
        .expect("found there");
    assert_eq!(created, Some(first));
    assert_eq!(again, None);
    assert_eq!(
        companion_rows(&schema, organisation, id).await,
        [("nl".to_owned(), "Open".to_owned())]
    );

    let mut edited = notice(organisation, id, "Open to all", "Open voor iedereen");
    edited.title.set("de", "Offen für alle");
    let updated = fundamental_db::Repository::update(&repository, &edited)
        .await
        .expect("updated");
    assert_eq!(updated, edited);
    assert_eq!(
        repository.find_by_key(&key).await.expect("read"),
        Some(edited)
    );

    let upserted = notice(organisation, id, "Closed", "Gesloten");
    fundamental_db::Repository::upsert(&repository, &upserted)
        .await
        .expect("upserted");
    assert_eq!(
        companion_rows(&schema, organisation, id).await,
        [("nl".to_owned(), "Gesloten".to_owned())],
        "the language the upsert dropped is gone"
    );

    assert!(repository.delete_by_key(&key).await.expect("deleted"));
    assert!(companion_rows(&schema, organisation, id).await.is_empty());
}

#[tokio::test]
async fn an_update_of_one_organisations_record_leaves_another_organisations_same_id_alone() {
    let schema = notices().await;
    let (first, second, id) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let ours = notice(first, id, "Open", "Open bij ons");
    let theirs = notice(second, id, "Open", "Open bij hen");
    let edited = Notice {
        title: Localized::new("Closed"),
        ..ours.clone()
    };
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.insert(&ours).await?;
            tx.insert(&theirs).await?;
            tx.update(&edited).await?;
            tx.upsert(&edited).await
        })
        .await
        .expect("written and edited");

    assert!(companion_rows(&schema, first, id).await.is_empty());
    assert_eq!(
        companion_rows(&schema, second, id).await,
        [("nl".to_owned(), "Open bij hen".to_owned())],
        "an edit of our record never reaches theirs"
    );
}

#[tokio::test]
async fn a_record_deleted_outright_can_be_written_again_under_its_key() {
    let schema = notices().await;
    let (organisation, id) = (Uuid::new_v4(), Uuid::new_v4());
    let record = notice(organisation, id, "Open", "Open voor iedereen");
    let key = (organisation, id);
    let again = schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.insert(&record).await?;
            SqlModelQuery::<Notice>::delete_by_key(&mut *tx, &key).await?;
            tx.insert(&record).await?;
            SqlModelQuery::<Notice>::new(&mut *tx)
                .pin_key(organisation)
                .filter(fundamental_db::Filter::eq("id", id))
                .delete()
                .await?;
            tx.insert_if_absent(&record).await?;
            tx.find_by_key::<Notice>(&key).await
        })
        .await
        .expect("deleted and written again");

    assert_eq!(again, Some(record));
    assert_eq!(
        companion_rows(&schema, organisation, id).await,
        [("nl".to_owned(), "Open voor iedereen".to_owned())]
    );
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql, table = "leaflets", soft_delete)]
struct Leaflet {
    #[model(id)]
    id: Uuid,
    summary: Localized<String>,
}

fn leaflet(english: &str, dutch: &str) -> Leaflet {
    Leaflet {
        id: Uuid::new_v4(),
        summary: Localized::new(english).with("nl", dutch),
    }
}

async fn leaflets() -> SqlSchema {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let own = <Leaflet as ModelSchema<SqlDb>>::table_schema();
    let companion =
        <Leaflet as ModelSchema<SqlDb>>::translation_schema().expect("a localized record");
    for statement in render::create_all(&own)
        .into_iter()
        .chain(render::create_all(&companion))
    {
        schema
            .pool()
            .execute_ddl(&statement)
            .await
            .expect("the tables exist");
    }
    schema
}

#[tokio::test]
async fn a_record_keyed_by_its_id_reads_and_deletes_with_its_translations_on_the_pool() {
    use fundamental_db::{
        KeyedRepository as _, KeyedRepositoryBatch as _, RepositorySoftDelete as _,
    };
    let schema = leaflets().await;
    let repository = schema.pool().repository::<Leaflet>();
    let (one, two) = (leaflet("One", "Een"), leaflet("Two", "Twee"));
    for written in [&one, &two] {
        fundamental_db::Repository::insert(&repository, written)
            .await
            .expect("written");
    }

    assert_eq!(
        repository.find(&one.id).await.expect("found"),
        Some(one.clone())
    );
    let mut many = repository
        .find_many(&[one.id, two.id])
        .await
        .expect("found both");
    many.sort_by_key(|found| found.summary.resolve(&nl()).to_owned());
    assert_eq!(many, [one.clone(), two.clone()]);

    assert!(repository.soft_delete(&one.id).await.expect("set aside"));
    assert_eq!(repository.find(&one.id).await.expect("read"), None);
    assert_eq!(
        repository.find_with_deleted(&one.id).await.expect("read"),
        Some(one.clone()),
        "a record set aside keeps its translations"
    );

    assert!(repository.delete(&one.id).await.expect("deleted"));
    assert_eq!(repository.delete_many(&[two.id]).await.expect("deleted"), 1);
    let left = schema
        .pool()
        .fetch_all("SELECT id FROM leaflets_i18n")
        .await
        .expect("the companion");
    assert!(
        left.is_empty(),
        "every deleted record's translations go with it"
    );
    fundamental_db::Repository::insert(&repository, &one)
        .await
        .expect("the same id is written again");
}

#[tokio::test]
async fn two_edits_of_one_record_at_once_leave_the_whole_of_the_last_one() {
    let schema = notices().await;
    let (organisation, id) = (Uuid::new_v4(), Uuid::new_v4());
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.insert(&notice(organisation, id, "Open", "Open")).await
        })
        .await
        .expect("written");
    let mut held = notice(organisation, id, "Held", "Vast");
    held.title.set("de", "Gehalten");
    let last = notice(organisation, id, "Last", "Laatst");

    let wrote = tokio::sync::Notify::new();
    let first = schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.update(&held).await?;
            wrote.notify_one();
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            Ok::<_, DatabaseError>(())
        });
    let second = async {
        wrote.notified().await;
        schema
            .pool()
            .transaction(IsolationLevel::ReadCommitted, async |tx| {
                tx.update(&last).await
            })
            .await
    };
    let (first, second) = tokio::join!(first, second);
    first.expect("the first edit");
    second.expect("the second edit, after the first");

    assert_eq!(
        companion_rows(&schema, organisation, id).await,
        [("nl".to_owned(), "Laatst".to_owned())],
        "no language of the first edit outlives the second"
    );
}

#[tokio::test]
async fn a_record_and_its_translations_are_read_in_one_statement() {
    let schema = notices().await;
    let organisation = Uuid::new_v4();
    let written = three_notices(&schema, organisation).await;
    let first = written.first().expect("a notice").clone();
    let key = (organisation, first.id);

    let statements = Statements::default();
    let _installed = tracing_subscriber::registry()
        .with(statements.clone())
        .set_default();
    let by_position = || {
        QueryBuilder::<Notice, SqlDb>::new()
            .pin_key(organisation)
            .order_by(OrderBy::asc("position"))
    };
    let reads = schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            statements.run();
            let found = tx.find_by_key::<Notice>(&key).await?;
            let found_in = statements.run();
            let locked = tx.find_for_update_by_key::<Notice>(&key).await?;
            let locked_in = statements.run();
            let filtered = tx.query_where(&by_position()).await?;
            let filtered_in = statements.run();
            Ok::<_, DatabaseError>([
                (Vec::from_iter(found), found_in),
                (Vec::from_iter(locked), locked_in),
                (filtered, filtered_in),
            ])
        })
        .await
        .expect("read in a transaction");
    for (rows, statements_run) in reads {
        assert!(rows.contains(&first), "{rows:?}");
        assert_eq!(
            statements_run, 1,
            "the rows and their translations are one read"
        );
    }

    statements.run();
    let repository = schema.pool().repository::<Notice>();
    let found = repository.find_by_key(&key).await.expect("read");
    assert_eq!((found, statements.run()), (Some(first.clone()), 1));
    let page = fundamental_db::QueryExecutor::execute(
        &schema.pool().query_executor::<Notice>(),
        &by_position(),
    )
    .await
    .expect("read");
    assert_eq!((page, statements.run()), (written, 1));
    let by_key = SqlModelQuery::<Notice>::find_by_key(schema.pool(), &key)
        .await
        .expect("read");
    assert_eq!((by_key, statements.run()), (Some(first), 1));
}

#[tokio::test]
async fn what_is_to_check_in_either_language_is_kept_with_the_record() {
    let schema = notices().await;
    let (organisation, id) = (Uuid::new_v4(), Uuid::new_v4());
    let both = fundamental_core::i18n::Languages::new("en".parse().expect("en"), [nl()]);
    let key = (organisation, id);
    let read = async || {
        SqlModelQuery::<Notice>::find_by_key(schema.pool(), &key)
            .await
            .expect("read")
            .expect("there")
    };
    let mut record = notice(organisation, id, "Open", "Open voor iedereen");
    record.title.set_source("Open to everyone");
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.insert(&record).await
        })
        .await
        .expect("written");
    let mut stored = read().await;
    assert_eq!(stored.title.to_check(&both), [nl()], "the English changed");

    stored.title.set("nl", "Open voor iedereen, nu");
    stored.title.set("nl", "Voor iedereen open");
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.update(&stored).await
        })
        .await
        .expect("rewritten");
    assert_eq!(
        read().await.title.to_check(&both),
        [both.source().clone()],
        "the Dutch answered the English, then changed on its own"
    );
}

#[tokio::test]
async fn who_confirmed_a_text_still_fits_and_when_is_kept_with_the_record() {
    let schema = notices().await;
    let (organisation, id) = (Uuid::new_v4(), Uuid::new_v4());
    let both = fundamental_core::i18n::Languages::new("en".parse().expect("en"), [nl()]);
    let checked = fundamental_core::i18n::Checked::new("ann@example.com", 1_763_900_000_123);
    let mut record = notice(organisation, id, "Open", "Open voor iedereen");
    record.title.set_source("Open to everyone");
    assert!(record.title.still_fits(&nl(), &both, checked.clone()));
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.insert(&record).await
        })
        .await
        .expect("written");

    let stored = SqlModelQuery::<Notice>::find_by_key(schema.pool(), &(organisation, id))
        .await
        .expect("read")
        .expect("there");
    assert!(stored.title.to_check(&both).is_empty());
    assert_eq!(stored.title.checked(&nl()), Some(&checked));
    assert_eq!(stored, record);
}

/// Three notices whose English and Dutch sort differently; the third has no Dutch.
async fn three_fruits(schema: &SqlSchema, organisation: Uuid) -> [Notice; 3] {
    let written = [
        notice(organisation, Uuid::new_v4(), "Apple", "Zon"),
        notice(organisation, Uuid::new_v4(), "Banana", "Maan"),
        Notice {
            title: Localized::new("Cherry"),
            ..notice(organisation, Uuid::new_v4(), "", "")
        },
    ];
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            for row in &written {
                tx.insert(row).await?;
            }
            Ok::<_, DatabaseError>(())
        })
        .await
        .expect("written");
    written
}

fn dutch_titles(rows: &[Notice]) -> Vec<String> {
    rows.iter()
        .map(|row| row.title.resolve(&nl()).clone())
        .collect()
}

#[tokio::test]
async fn records_are_sorted_and_paged_by_their_text_in_the_readers_language() {
    let schema = notices().await;
    let organisation = Uuid::new_v4();
    let written = three_fruits(&schema, organisation).await;
    let in_dutch = || {
        SqlModelQuery::<Notice>::new(schema.pool())
            .pin_key(organisation)
            .in_language(&nl())
            .order_by(OrderBy::asc("title"))
            .order_by(OrderBy::asc("id"))
            .page_size(2)
    };

    let first = in_dutch().execute_page(None).await.expect("a page");
    assert_eq!(dutch_titles(&first.items), ["Cherry", "Maan"]);
    let second = in_dutch()
        .execute_page(first.next_page)
        .await
        .expect("the next page");
    assert_eq!(
        dutch_titles(&second.items),
        ["Zon"],
        "the page resumes the Dutch order"
    );
    assert_eq!(
        second.items[0], written[0],
        "a record read in a language is the record"
    );
}

#[tokio::test]
async fn records_are_searched_and_counted_by_their_text_in_the_readers_language() {
    let schema = notices().await;
    let organisation = Uuid::new_v4();
    three_fruits(&schema, organisation).await;
    let found = || {
        SqlModelQuery::<Notice>::new(schema.pool())
            .pin_key(organisation)
            .in_language(&nl())
            .filter(fundamental_db::Filter::icontains("title", "aan"))
    };
    assert_eq!(
        dutch_titles(&found().execute_page(None).await.expect("found").items),
        ["Maan"]
    );
    assert_eq!(found().count().await.expect("counted"), 1);
    let english = SqlModelQuery::<Notice>::new(schema.pool())
        .pin_key(organisation)
        .filter(fundamental_db::Filter::icontains("title", "nan"))
        .count()
        .await
        .expect("counted");
    assert_eq!(english, 1, "Banana, in its source");
}
