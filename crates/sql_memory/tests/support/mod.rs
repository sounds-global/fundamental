//! The conformance suite, run on the memory repositories and on YugabyteDB's.

use chrono::{DateTime, NaiveDate, TimeZone as _, Utc};
use fundamental_core::i18n::{Checked, Languages, Localized};
use fundamental_core::model::{BoundTo, SqlDb, SqlModel as _};
use fundamental_db::{
    Filter, KeyedRepository, OrderBy, QueryBuilder, QueryExecutor, Repository, RepositorySoftDelete,
};
use fundamental_macros::{Model, Variant};
use fundamental_sql::{Repositories, ViolatedConstraint};
use uuid::Uuid;

/// A row keyed by one column, with a column of each kind a filter compares.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql, table = "conformance_notes")]
pub struct Note {
    #[model(id)]
    pub id: Uuid,
    pub title: String,
    pub rank: i64,
    pub aside: Option<String>,
}

impl Note {
    pub fn new(title: &str, rank: i64) -> Self {
        Self {
            id: Uuid::new_v4(),
            title: title.to_owned(),
            rank,
            aside: None,
        }
    }

    pub fn aside(self, aside: &str) -> Self {
        Self {
            aside: Some(aside.to_owned()),
            ..self
        }
    }
}

pub async fn holding<M, const N: usize>(
    repository: &impl Repository<M, SqlDb>,
    rows: [M; N],
) -> [M; N]
where
    M: BoundTo<SqlDb> + Send + Sync,
{
    for row in &rows {
        repository.insert(row).await.expect("insert");
    }
    rows
}

/// A soft-deleted row.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql, table = "conformance_drafts", soft_delete)]
pub struct Draft {
    #[model(id)]
    pub id: Uuid,
    pub title: String,
}

impl Draft {
    pub fn new(title: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            title: title.to_owned(),
        }
    }
}

pub trait Drafts:
    KeyedRepository<Draft, SqlDb, Id = Uuid>
    + RepositorySoftDelete<Draft, SqlDb, Id = Uuid>
    + QueryExecutor<Draft, SqlDb>
{
}

impl<R> Drafts for R where
    R: KeyedRepository<Draft, SqlDb, Id = Uuid>
        + RepositorySoftDelete<Draft, SqlDb, Id = Uuid>
        + QueryExecutor<Draft, SqlDb>
{
}

/// A row keyed by its organisation first, then its own id, as a tenant's records are.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql, table = "conformance_entries")]
pub struct Entry {
    #[model(id, shard = hash)]
    pub organisation: Uuid,
    #[model(id)]
    pub id: Uuid,
    pub title: String,
}

pub trait Entries: Repository<Entry, SqlDb> + QueryExecutor<Entry, SqlDb> {}

impl<R: Repository<Entry, SqlDb> + QueryExecutor<Entry, SqlDb>> Entries for R {}

/// A closed value, stored as its key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Variant)]
#[variant(key = "conformance_kind")]
pub enum Kind {
    Plain,
    InUse,
}

/// A row with a text in several languages, a closed value, a day, a moment and a flag.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql, table = "conformance_pieces")]
pub struct Piece {
    #[model(id)]
    pub id: Uuid,
    pub title: Localized<String>,
    #[model(variant)]
    pub kind: Kind,
    pub day: Option<NaiveDate>,
    pub at: DateTime<Utc>,
    pub shown: bool,
}

pub trait Pieces: KeyedRepository<Piece, SqlDb, Id = Uuid> + QueryExecutor<Piece, SqlDb> {}

impl<R: KeyedRepository<Piece, SqlDb, Id = Uuid> + QueryExecutor<Piece, SqlDb>> Pieces for R {}

/// A row named once among its organisation's live rows, with an optional unique code.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(
    sql,
    table = "conformance_slugs",
    soft_delete,
    index(
        name = "conformance_slugs_by_slug",
        columns = ["organisation hash", "slug asc"],
        unique,
        partial = "deleted_at IS NULL"
    ),
    index(name = "conformance_slugs_by_code", columns = ["code asc"], unique)
)]
pub struct Slugged {
    #[model(id)]
    pub id: Uuid,
    pub organisation: Uuid,
    pub slug: String,
    pub code: Option<String>,
}

impl Slugged {
    pub fn new(organisation: Uuid, slug: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            organisation,
            slug: slug.to_owned(),
            code: None,
        }
    }

    pub fn code(self, code: &str) -> Self {
        Self {
            code: Some(code.to_owned()),
            ..self
        }
    }
}

pub trait Slugs:
    KeyedRepository<Slugged, SqlDb, Id = Uuid>
    + RepositorySoftDelete<Slugged, SqlDb, Id = Uuid>
    + QueryExecutor<Slugged, SqlDb>
{
}

impl<R> Slugs for R where
    R: KeyedRepository<Slugged, SqlDb, Id = Uuid>
        + RepositorySoftDelete<Slugged, SqlDb, Id = Uuid>
        + QueryExecutor<Slugged, SqlDb>
{
}

/// Whether `written` was refused as a second row of the unique index `index`.
#[track_caller]
pub fn refused_by<T: std::fmt::Debug>(written: fundamental_db::DatabaseResult<T>, index: &str) {
    let error = written.expect_err(index);
    let violated = ViolatedConstraint::of(&error).expect("a violated constraint");
    assert!(violated.is_unique(index), "{index}: {violated:?}");
}

pub trait Notes: KeyedRepository<Note, SqlDb, Id = Uuid> + QueryExecutor<Note, SqlDb> {}

impl<R: KeyedRepository<Note, SqlDb, Id = Uuid> + QueryExecutor<Note, SqlDb>> Notes for R {}

pub async fn a_row_inserted_is_found_by_its_id(notes: impl Notes) {
    let note = Note::new("First", 1);
    notes.insert(&note).await.expect("insert");
    assert_eq!(notes.find(&note.id).await.expect("find"), Some(note));
    assert_eq!(notes.find(&Uuid::new_v4()).await.expect("find"), None);
}

pub async fn a_second_row_of_one_key_is_refused_as_its_primary_key(notes: impl Notes) {
    let note = Note::new("First", 1);
    notes.insert(&note).await.expect("insert");
    let again = Note {
        title: "Again".to_owned(),
        ..note.clone()
    };
    let error = notes.insert(&again).await.expect_err("one row a key");
    let violated = ViolatedConstraint::of(&error).expect("a violated constraint");
    assert!(violated.is_unique("conformance_notes_pkey"), "{violated:?}");
    assert_eq!(notes.find(&note.id).await.expect("find"), Some(note));
}

pub async fn an_update_replaces_the_row_its_key_names(notes: impl Notes) {
    let note = Note::new("First", 1);
    let other = Note::new("Other", 2);
    notes.insert(&note).await.expect("insert");
    notes.insert(&other).await.expect("insert");
    let changed = Note {
        title: "Changed".to_owned(),
        rank: 5,
        ..note.clone()
    };
    assert_eq!(notes.update(&changed).await.expect("update"), changed);
    assert_eq!(notes.find(&note.id).await.expect("find"), Some(changed));
    assert_eq!(notes.find(&other.id).await.expect("find"), Some(other));

    let absent = Note::new("Absent", 3);
    notes
        .update(&absent)
        .await
        .expect_err("an update writes no new row");
    assert_eq!(notes.find(&absent.id).await.expect("find"), None);
}

pub async fn an_upsert_writes_a_row_or_replaces_the_one_its_key_names(notes: impl Notes) {
    let note = Note::new("First", 1);
    assert_eq!(notes.upsert(&note).await.expect("upsert"), note);
    let changed = Note {
        title: "Changed".to_owned(),
        ..note.clone()
    };
    assert_eq!(notes.upsert(&changed).await.expect("upsert"), changed);
    assert_eq!(notes.find(&note.id).await.expect("find"), Some(changed));
}

pub async fn a_row_deleted_is_gone_and_said_once(notes: impl Notes) {
    let [note, other] = holding(&notes, [Note::new("First", 1), Note::new("Other", 2)]).await;
    assert!(notes.exists(&note.id).await.expect("exists"));

    let said = [
        notes.delete(&note.id).await.expect("delete"),
        notes.delete(&note.id).await.expect("delete"),
    ];
    assert_eq!(said, [true, false], "deleted once");
    assert!(!notes.exists(&note.id).await.expect("exists"));
    assert_eq!(notes.find(&note.id).await.expect("find"), None);
    let required = notes.find_required(&note.id).await;
    assert!(required.is_err(), "a required row that is gone is an error");
    assert_eq!(notes.find_required(&other.id).await.expect("find"), other);
}

async fn kept(notes: &impl Notes, filter: Filter) -> Vec<String> {
    let mut titles: Vec<String> = notes
        .execute(&notes.query().filter(filter))
        .await
        .expect("execute")
        .into_iter()
        .map(|note| note.title)
        .collect();
    titles.sort();
    titles
}

pub async fn a_query_keeps_the_rows_a_comparison_matches(notes: impl Notes) {
    holding(
        &notes,
        [
            Note::new("a", 1),
            Note::new("b", 2).aside("seen"),
            Note::new("c", 3),
        ],
    )
    .await;
    answers(
        &notes,
        [
            (Filter::eq("rank", 2), &["b"][..]),
            (Filter::ne("rank", 2), &["a", "c"]),
            (Filter::gt("rank", 2), &["c"]),
            (Filter::gte("rank", 2), &["b", "c"]),
            (Filter::lt("rank", 2), &["a"]),
            (Filter::lte("rank", 2), &["a", "b"]),
            (Filter::between("rank", 2, 3), &["b", "c"]),
            (Filter::eq("title", "c"), &["c"]),
            // NULL is no answer, so a row without an aside is neither equal nor unequal.
            (Filter::ne("aside", "seen"), &[]),
        ],
    )
    .await;
}

async fn answers<const N: usize>(notes: &impl Notes, cases: [(Filter, &[&str]); N]) {
    for (filter, titles) in cases {
        let said = format!("{filter:?}");
        assert_eq!(kept(notes, filter).await, titles, "{said}");
    }
}

pub async fn a_query_keeps_the_rows_a_set_a_null_or_a_pattern_matches(notes: impl Notes) {
    holding(
        &notes,
        [
            Note::new("Alpha", 1),
            Note::new("beta", 2).aside("seen"),
            Note::new("100%", 3),
        ],
    )
    .await;
    answers(
        &notes,
        [
            (
                Filter::in_list("rank", vec![1.into(), 3.into()]),
                &["100%", "Alpha"][..],
            ),
            (Filter::not_in("rank", vec![1.into(), 3.into()]), &["beta"]),
            (Filter::is_null("aside"), &["100%", "Alpha"]),
            (Filter::is_not_null("aside"), &["beta"]),
            (Filter::like("title", "%eta"), &["beta"]),
            (Filter::like("title", "a%"), &[]),
            (Filter::like("title", "_lpha"), &["Alpha"]),
            (Filter::ilike("title", "a%"), &["Alpha"]),
            (Filter::icontains("title", "0%"), &["100%"]),
            (Filter::icontains("title", "LP"), &["Alpha"]),
        ],
    )
    .await;
}

pub async fn a_query_keeps_the_rows_a_combination_matches(notes: impl Notes) {
    holding(
        &notes,
        [
            Note::new("a", 1),
            Note::new("b", 2).aside("seen"),
            Note::new("c", 2),
        ],
    )
    .await;
    let two = || Filter::eq("rank", 2);
    let seen = || Filter::eq("aside", "seen");
    let after = |rank: i64, title: &str| {
        Filter::row_value_gt(
            vec!["rank".to_owned(), "title".to_owned()],
            vec![rank.into(), title.into()],
        )
    };
    answers(
        &notes,
        [
            (Filter::and(vec![two(), seen()]), &["b"][..]),
            (Filter::or(vec![Filter::eq("rank", 1), seen()]), &["a", "b"]),
            (Filter::not(Filter::eq("rank", 1)), &["b", "c"]),
            // NOT of no answer is no answer: a note without an aside is not kept either way.
            (Filter::not(seen()), &[]),
            (
                Filter::or(vec![seen(), Filter::eq("title", "c")]),
                &["b", "c"],
            ),
            (after(2, "b"), &["c"]),
            (after(1, "z"), &["b", "c"]),
            (
                Filter::row_value_lt(
                    vec!["rank".to_owned(), "title".to_owned()],
                    vec![2.into(), "c".into()],
                ),
                &["a", "b"],
            ),
        ],
    )
    .await;
}

pub async fn a_query_reads_its_rows_in_order_a_page_at_a_time(notes: impl Notes) {
    holding(&notes, four()).await;
    let by_rank = || {
        notes
            .query()
            .order_by(OrderBy::desc("rank"))
            .order_by(OrderBy::asc("title"))
    };
    assert_eq!(titles(&notes, by_rank()).await, ["d", "a", "c", "b"]);
    assert_eq!(titles(&notes, by_rank().limit(2)).await, ["d", "a"]);
    assert_eq!(
        titles(&notes, by_rank().limit(2).offset(2)).await,
        ["c", "b"]
    );
    assert_eq!(titles(&notes, by_rank().offset(3)).await, ["b"]);
    let first = notes.execute_one(&by_rank()).await.expect("execute one");
    assert_eq!(first.map(|note| note.title).as_deref(), Some("d"));
}

pub async fn a_null_sorts_last_going_up_and_first_going_down(notes: impl Notes) {
    holding(&notes, four()).await;
    let by_aside = |order: OrderBy| {
        notes
            .query()
            .order_by(order)
            .order_by(OrderBy::asc("title"))
    };
    assert_eq!(
        titles(&notes, by_aside(OrderBy::asc("aside"))).await,
        ["b", "c", "a", "d"]
    );
    assert_eq!(
        titles(&notes, by_aside(OrderBy::desc("aside"))).await,
        ["a", "d", "c", "b"]
    );
    assert_eq!(
        titles(&notes, by_aside(OrderBy::asc("aside").nulls_first())).await,
        ["a", "d", "b", "c"]
    );
}

pub async fn a_count_counts_every_row_its_filters_keep(notes: impl Notes) {
    holding(&notes, four()).await;
    let two = notes.query().filter(Filter::eq("rank", 2));
    assert_eq!(notes.count(&two).await.expect("count"), 2);
    assert_eq!(notes.count(&notes.query()).await.expect("count"), 4);
}

fn four() -> [Note; 4] {
    [
        Note::new("a", 2),
        Note::new("b", 1).aside("x"),
        Note::new("c", 2).aside("y"),
        Note::new("d", 3),
    ]
}

async fn titles(notes: &impl Notes, query: QueryBuilder<Note, SqlDb>) -> Vec<String> {
    let found = notes.execute(&query).await.expect("execute");
    found.into_iter().map(|note| note.title).collect()
}

pub async fn a_row_deleted_softly_is_read_only_when_asked_for(drafts: impl Drafts) {
    let [kept, deleted] = holding(&drafts, [Draft::new("kept"), Draft::new("deleted")]).await;
    let said = [
        drafts.soft_delete(&deleted.id).await.expect("soft delete"),
        drafts.soft_delete(&deleted.id).await.expect("soft delete"),
    ];
    assert_eq!(said, [true, false], "deleted once");
    assert_eq!(drafts.find(&deleted.id).await.expect("find"), None);
    assert!(!drafts.exists(&deleted.id).await.expect("exists"));
    let all = drafts.execute(&drafts.query()).await.expect("execute");
    assert_eq!(all, [kept]);
    let gone = drafts.query().filter(Filter::is_not_null("deleted_at"));
    assert_eq!(drafts.count(&gone).await.expect("count"), 0);
    let found = drafts.find_with_deleted(&deleted.id).await.expect("find");
    assert_eq!(found, Some(deleted));
}

pub async fn a_row_deleted_softly_is_restored_or_purged(drafts: impl Drafts) {
    let [draft] = holding(&drafts, [Draft::new("draft")]).await;
    drafts.soft_delete(&draft.id).await.expect("soft delete");
    let again = drafts.insert(&draft).await;
    assert!(again.is_err(), "its key is still held");
    assert!(drafts.restore(&draft.id).await.expect("restore"));
    assert_eq!(
        drafts.find(&draft.id).await.expect("find"),
        Some(draft.clone())
    );
    drafts.soft_delete(&draft.id).await.expect("soft delete");
    assert!(drafts.purge(&draft.id).await.expect("purge"));
    let found = drafts.find_with_deleted(&draft.id).await.expect("find");
    assert_eq!(found, None);
}

pub async fn a_delete_by_filter_removes_the_rows_it_keeps(notes: impl Notes) {
    holding(&notes, four()).await;
    let two = notes.query().filter(Filter::eq("rank", 2));
    assert_eq!(notes.delete_where(&two).await.expect("delete"), 2);
    assert_eq!(
        titles(&notes, notes.query().order_by(OrderBy::asc("title"))).await,
        ["b", "d"]
    );
    let first = notes.query().order_by(OrderBy::asc("title")).limit(1);
    assert_eq!(
        notes.delete_where(&first).await.expect("delete"),
        1,
        "at most its limit"
    );
    assert_eq!(titles(&notes, notes.query()).await, ["d"]);
}

pub async fn a_row_is_one_organisations_by_its_leading_key(entries: impl Entries) {
    let (ours, theirs, id) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let entry = |organisation, id, title: &str| Entry {
        organisation,
        id,
        title: title.to_owned(),
    };
    holding(
        &entries,
        [
            entry(ours, id, "ours"),
            entry(ours, Uuid::new_v4(), "also ours"),
            entry(theirs, id, "theirs"),
        ],
    )
    .await;
    let again = entries.insert(&entry(ours, id, "again")).await;
    assert!(again.is_err(), "one row a whole key");
    entries
        .update(&entry(ours, id, "changed"))
        .await
        .expect("update");
    let read = async |organisation: Uuid| -> Vec<String> {
        let query = entries
            .query()
            .pin_key(organisation)
            .order_by(OrderBy::asc("title"));
        let found = entries.execute(&query).await.expect("execute");
        found.into_iter().map(|entry| entry.title).collect()
    };
    assert_eq!(read(ours).await, ["also ours", "changed"]);
    assert_eq!(read(theirs).await, ["theirs"]);
    let gone = entries.delete_where(&entries.query().pin_key(theirs)).await;
    assert_eq!(gone.expect("delete"), 1);
    assert_eq!(read(ours).await, ["also ours", "changed"]);
}

pub async fn a_row_reads_back_with_every_value_it_was_written_with(pieces: impl Pieces) {
    let moment = |hour| {
        Utc.with_ymd_and_hms(2026, 10, 1, hour, 0, 0)
            .single()
            .expect("a moment")
    };
    let [one, two] = holding(
        &pieces,
        [
            Piece {
                id: Uuid::new_v4(),
                title: Localized::new("One".to_owned()).with("nl", "Een".to_owned()),
                kind: Kind::Plain,
                day: NaiveDate::from_ymd_opt(2026, 10, 1),
                at: moment(9),
                shown: true,
            },
            Piece {
                id: Uuid::new_v4(),
                title: Localized::new("Two".to_owned()),
                kind: Kind::InUse,
                day: None,
                at: moment(10),
                shown: false,
            },
        ],
    )
    .await;
    assert_eq!(pieces.find(&one.id).await.expect("find"), Some(one.clone()));
    let read = async |query: QueryBuilder<Piece, SqlDb>| -> Vec<Uuid> {
        let found = pieces.execute(&query).await.expect("execute");
        found.into_iter().map(|piece| piece.id).collect()
    };
    let by = |filter: Filter| pieces.query().filter(filter);
    assert_eq!(read(by(Filter::eq("kind", "in_use"))).await, [two.id]);
    assert_eq!(read(by(Filter::eq("shown", true))).await, [one.id]);
    assert_eq!(read(by(Filter::gt("at", moment(9)))).await, [two.id]);
    assert_eq!(
        read(by(Filter::eq("title", "One"))).await,
        [one.id],
        "the source text"
    );
    let by_day = pieces.query().order_by(OrderBy::asc("day"));
    assert_eq!(read(by_day).await, [one.id, two.id]);
}

pub async fn a_column_the_table_lacks_is_refused(notes: impl Notes) {
    holding(&notes, [Note::new("a", 1)]).await;
    let filtered = notes.query().filter(Filter::eq("nowhere", 1));
    assert!(notes.execute(&filtered).await.is_err(), "a filter on it");
    let ordered = notes.query().order_by(OrderBy::asc("nowhere"));
    assert!(notes.execute(&ordered).await.is_err(), "an order by it");
}

pub async fn a_database_hands_out_each_models_repository(database: impl Repositories) {
    let [note] = holding(&database.repository::<Note>(), [Note::new("a", 1)]).await;
    let entry = Entry {
        organisation: Uuid::new_v4(),
        id: Uuid::new_v4(),
        title: "entry".to_owned(),
    };
    holding(&database.repository::<Entry>(), [entry.clone()]).await;
    let again = database.clone();
    let notes = again.repository::<Note>();
    assert_eq!(
        notes.execute(&notes.query()).await.expect("execute"),
        [note]
    );
    let entries = again.repository::<Entry>();
    let theirs = entries.query().pin_key(entry.organisation);
    assert_eq!(entries.execute(&theirs).await.expect("execute"), [entry]);
}

pub async fn a_unique_index_refuses_a_second_row_of_its_columns(slugs: impl Slugs) {
    let (ours, theirs) = (Uuid::new_v4(), Uuid::new_v4());
    let [acme, shop] = holding(
        &slugs,
        [Slugged::new(ours, "acme"), Slugged::new(ours, "shop")],
    )
    .await;
    let by_slug = "conformance_slugs_by_slug";
    refused_by(slugs.insert(&Slugged::new(ours, "acme")).await, by_slug);
    slugs
        .insert(&Slugged::new(theirs, "acme"))
        .await
        .expect("another organisation's");

    let taking = Slugged {
        slug: "acme".to_owned(),
        ..shop.clone()
    };
    refused_by(slugs.update(&taking).await, by_slug);
    refused_by(slugs.upsert(&taking).await, by_slug);
    assert_eq!(slugs.find(&shop.id).await.expect("find"), Some(shop));
    let kept = acme.clone().code("A");
    slugs.update(&kept).await.expect("its own name kept");
    slugs.upsert(&kept).await.expect("its own name kept");
}

pub async fn a_unique_index_holds_the_rows_its_condition_keeps_and_no_null(slugs: impl Slugs) {
    let ours = Uuid::new_v4();
    let [first] = holding(&slugs, [Slugged::new(ours, "acme").code("A")]).await;
    slugs.soft_delete(&first.id).await.expect("soft delete");
    let second = Slugged::new(ours, "acme");
    slugs
        .insert(&second)
        .await
        .expect("a name a deleted row held is free");
    refused_by(slugs.restore(&first.id).await, "conformance_slugs_by_slug");

    let by_code = "conformance_slugs_by_code";
    refused_by(
        slugs
            .insert(&Slugged::new(ours, "fundamental").code("A"))
            .await,
        by_code,
    );
    holding(
        &slugs,
        [Slugged::new(ours, "widgets"), Slugged::new(ours, "gadgets")],
    )
    .await;
}

async fn refused<M, R>(repository: &R, query: QueryBuilder<M, SqlDb>) -> bool
where
    M: BoundTo<SqlDb> + Send + Sync,
    R: QueryExecutor<M, SqlDb>,
    fundamental_core::model::LeadingKeyAddressed<M, SqlDb, fundamental_core::model::NoLeadingKey>:
        fundamental_core::model::Satisfied,
{
    repository.execute(&query).await.is_err()
}

pub async fn a_filter_its_column_cannot_take_is_refused(notes: impl Notes) {
    let unrelated = || {
        [
            Filter::eq("rank", "abc"),
            Filter::gt("rank", true),
            Filter::eq("title", 3),
            Filter::eq("id", "not a uuid"),
            Filter::eq("id", Uuid::new_v4().to_string()),
            Filter::in_list("rank", vec![1.into(), "two".into()]),
            Filter::like("id", "%a%"),
            Filter::eq("aside", None::<String>),
        ]
    };
    for filter in unrelated() {
        let said = format!("{filter:?}");
        assert!(
            refused(&notes, notes.query().filter(filter)).await,
            "on no rows: {said}"
        );
    }
    holding(&notes, [Note::new("a", 1).aside("seen")]).await;
    for filter in unrelated() {
        let said = format!("{filter:?}");
        assert!(
            refused(&notes, notes.query().filter(filter)).await,
            "{said}"
        );
    }
    let related = notes.query().filter(Filter::and(vec![
        Filter::gt("rank", 0.5),
        Filter::eq("aside", "seen"),
    ]));
    assert_eq!(titles(&notes, related).await, ["a"]);
}

pub async fn a_flag_a_day_and_a_moment_take_only_their_own_values(pieces: impl Pieces) {
    let moment = Utc
        .with_ymd_and_hms(2026, 10, 1, 9, 0, 0)
        .single()
        .expect("a moment");
    let [piece] = holding(
        &pieces,
        [Piece {
            id: Uuid::new_v4(),
            title: Localized::new("One".to_owned()),
            kind: Kind::Plain,
            day: NaiveDate::from_ymd_opt(2026, 10, 1),
            at: moment,
            shown: true,
        }],
    )
    .await;
    for filter in [
        Filter::eq("shown", 1),
        Filter::eq("day", "2026-10-01"),
        Filter::gt("at", "2026-10-01"),
    ] {
        let said = format!("{filter:?}");
        assert!(
            refused(&pieces, pieces.query().filter(filter)).await,
            "{said}"
        );
    }
    let before = pieces.query().filter(Filter::lt("day", moment));
    let found = pieces
        .execute(&before)
        .await
        .expect("a day before a moment");
    assert_eq!(
        found.into_iter().map(|piece| piece.id).collect::<Vec<_>>(),
        [piece.id],
        "the day is its first instant"
    );
}

pub async fn a_moment_reads_back_to_the_microsecond_it_is_kept_at(pieces: impl Pieces) {
    let at = |nanos| {
        Utc.timestamp_opt(1_790_000_000, nanos)
            .single()
            .expect("a moment")
    };
    let written = Piece {
        id: Uuid::new_v4(),
        title: Localized::new("One".to_owned()),
        kind: Kind::Plain,
        day: None,
        at: at(123_456_789),
        shown: true,
    };
    let kept = Piece {
        at: at(123_456_000),
        ..written.clone()
    };
    assert_eq!(pieces.insert(&written).await.expect("insert"), kept);
    assert_eq!(pieces.find(&written.id).await.expect("find"), Some(kept));

    let changed = Piece {
        at: at(987_654_321),
        ..written.clone()
    };
    let kept = Piece {
        at: at(987_654_000),
        ..written.clone()
    };
    assert_eq!(pieces.update(&changed).await.expect("update"), kept);
    assert_eq!(pieces.upsert(&changed).await.expect("upsert"), kept);
    let found = pieces.execute(&pieces.query()).await.expect("execute");
    assert_eq!(found, [kept]);
}

pub async fn a_checked_translation_reads_back_as_written(pieces: impl Pieces) {
    let languages = Languages::new(
        "en".parse().expect("a language"),
        ["nl".parse().expect("a language")],
    );
    let mut title = Localized::new("Open".to_owned()).with("nl", "Geopend".to_owned());
    title.set_source("Open now".to_owned());
    let checked = title.still_fits(
        &"nl".parse().expect("a language"),
        &languages,
        Checked::new("ann", 1_790_000_000_123),
    );
    assert!(checked, "the Dutch was to check against the new English");
    let written = Piece {
        id: Uuid::new_v4(),
        title,
        kind: Kind::Plain,
        day: None,
        at: Utc
            .timestamp_opt(1_790_000_000, 0)
            .single()
            .expect("a moment"),
        shown: true,
    };

    assert_eq!(pieces.insert(&written).await.expect("insert"), written);
    assert_eq!(
        pieces.find(&written.id).await.expect("find"),
        Some(written.clone().into_stored())
    );
    assert_eq!(
        written.clone().into_stored(),
        written,
        "nothing of it is cut"
    );
}
