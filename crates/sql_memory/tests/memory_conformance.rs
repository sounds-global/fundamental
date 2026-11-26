//! The memory repositories pass the conformance suite YugabyteDB's pass.

mod support;

use fundamental_sql_memory::MemoryDatabase;
use support::{Draft, Entry, Note, Piece, Slugged};

#[tokio::test]
async fn a_row_inserted_is_found_by_its_id() {
    support::a_row_inserted_is_found_by_its_id(MemoryDatabase::new().repository::<Note>()).await;
}

#[tokio::test]
async fn a_second_row_of_one_key_is_refused_as_its_primary_key() {
    support::a_second_row_of_one_key_is_refused_as_its_primary_key(
        MemoryDatabase::new().repository::<Note>(),
    )
    .await;
}

#[tokio::test]
async fn an_update_replaces_the_row_its_key_names() {
    support::an_update_replaces_the_row_its_key_names(MemoryDatabase::new().repository::<Note>())
        .await;
}

#[tokio::test]
async fn an_upsert_writes_a_row_or_replaces_the_one_its_key_names() {
    support::an_upsert_writes_a_row_or_replaces_the_one_its_key_names(
        MemoryDatabase::new().repository::<Note>(),
    )
    .await;
}

#[tokio::test]
async fn a_row_deleted_is_gone_and_said_once() {
    support::a_row_deleted_is_gone_and_said_once(MemoryDatabase::new().repository::<Note>()).await;
}

#[tokio::test]
async fn a_query_keeps_the_rows_a_comparison_matches() {
    support::a_query_keeps_the_rows_a_comparison_matches(
        MemoryDatabase::new().repository::<Note>(),
    )
    .await;
}

#[tokio::test]
async fn a_query_keeps_the_rows_a_set_a_null_or_a_pattern_matches() {
    support::a_query_keeps_the_rows_a_set_a_null_or_a_pattern_matches(
        MemoryDatabase::new().repository::<Note>(),
    )
    .await;
}

#[tokio::test]
async fn a_query_keeps_the_rows_a_combination_matches() {
    support::a_query_keeps_the_rows_a_combination_matches(
        MemoryDatabase::new().repository::<Note>(),
    )
    .await;
}

#[tokio::test]
async fn a_query_reads_its_rows_in_order_a_page_at_a_time() {
    support::a_query_reads_its_rows_in_order_a_page_at_a_time(
        MemoryDatabase::new().repository::<Note>(),
    )
    .await;
}

#[tokio::test]
async fn a_null_sorts_last_going_up_and_first_going_down() {
    support::a_null_sorts_last_going_up_and_first_going_down(
        MemoryDatabase::new().repository::<Note>(),
    )
    .await;
}

#[tokio::test]
async fn a_count_counts_every_row_its_filters_keep() {
    support::a_count_counts_every_row_its_filters_keep(MemoryDatabase::new().repository::<Note>())
        .await;
}

#[tokio::test]
async fn a_row_deleted_softly_is_read_only_when_asked_for() {
    support::a_row_deleted_softly_is_read_only_when_asked_for(
        MemoryDatabase::new().repository::<Draft>(),
    )
    .await;
}

#[tokio::test]
async fn a_row_deleted_softly_is_restored_or_purged() {
    support::a_row_deleted_softly_is_restored_or_purged(
        MemoryDatabase::new().repository::<Draft>(),
    )
    .await;
}

#[tokio::test]
async fn a_delete_by_filter_removes_the_rows_it_keeps() {
    support::a_delete_by_filter_removes_the_rows_it_keeps(
        MemoryDatabase::new().repository::<Note>(),
    )
    .await;
}

#[tokio::test]
async fn a_row_is_one_organisations_by_its_leading_key() {
    support::a_row_is_one_organisations_by_its_leading_key(
        MemoryDatabase::new().repository::<Entry>(),
    )
    .await;
}

#[tokio::test]
async fn a_row_reads_back_with_every_value_it_was_written_with() {
    support::a_row_reads_back_with_every_value_it_was_written_with(
        MemoryDatabase::new().repository::<Piece>(),
    )
    .await;
}

#[tokio::test]
async fn a_column_the_table_lacks_is_refused() {
    support::a_column_the_table_lacks_is_refused(MemoryDatabase::new().repository::<Note>()).await;
}

#[tokio::test]
async fn an_update_by_filter_is_refused_for_rows_held_whole() {
    use fundamental_db::{Filter, QueryExecutor as _, Repository as _};
    let notes = MemoryDatabase::new().repository::<Note>();
    notes.insert(&Note::new("a", 1)).await.expect("insert");
    let query = notes.query().filter(Filter::eq("rank", 1));
    let refused = notes
        .update_where(&query, &[("rank".to_owned(), 2.into())])
        .await;
    assert!(
        refused.is_err(),
        "the memory database holds models, not columns"
    );
}

/// A row whose unique index holds only the rows of a condition the memory database cannot read.
#[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
#[model(
    sql,
    table = "memory_ranked",
    index(name = "memory_ranked_high", columns = ["title asc"], unique, partial = "rank > 2")
)]
struct Ranked {
    #[model(id)]
    id: uuid::Uuid,
    title: String,
    rank: i64,
}

#[test]
fn a_model_lists_its_index_as_unique_and_partial() {
    use fundamental_core::model::{IndexDef, SqlModel};
    assert_eq!(
        <Ranked as SqlModel>::INDEXES,
        [IndexDef::new("memory_ranked_high", &["title"])
            .unique()
            .with_condition("rank > 2")]
    );
}

#[tokio::test]
async fn a_unique_index_it_cannot_read_refuses_the_model_at_its_first_write() {
    use fundamental_db::Repository as _;
    let ranked = MemoryDatabase::new().repository::<Ranked>();
    let row = Ranked {
        id: uuid::Uuid::new_v4(),
        title: "a".to_owned(),
        rank: 1,
    };
    let refused = ranked.insert(&row).await;
    assert!(
        refused.is_err(),
        "an index held loosely would answer unlike the database"
    );
}

#[tokio::test]
async fn a_database_hands_out_each_models_repository() {
    support::a_database_hands_out_each_models_repository(MemoryDatabase::new()).await;
}

#[tokio::test]
async fn a_unique_index_refuses_a_second_row_of_its_columns() {
    support::a_unique_index_refuses_a_second_row_of_its_columns(
        MemoryDatabase::new().repository::<Slugged>(),
    )
    .await;
}

#[tokio::test]
async fn a_unique_index_holds_the_rows_its_condition_keeps_and_no_null() {
    support::a_unique_index_holds_the_rows_its_condition_keeps_and_no_null(
        MemoryDatabase::new().repository::<Slugged>(),
    )
    .await;
}

#[tokio::test]
async fn a_filter_its_column_cannot_take_is_refused() {
    support::a_filter_its_column_cannot_take_is_refused(MemoryDatabase::new().repository::<Note>())
        .await;
}

#[tokio::test]
async fn a_flag_a_day_and_a_moment_take_only_their_own_values() {
    support::a_flag_a_day_and_a_moment_take_only_their_own_values(
        MemoryDatabase::new().repository::<Piece>(),
    )
    .await;
}

#[tokio::test]
async fn a_moment_reads_back_to_the_microsecond_it_is_kept_at() {
    support::a_moment_reads_back_to_the_microsecond_it_is_kept_at(
        MemoryDatabase::new().repository::<Piece>(),
    )
    .await;
}

#[tokio::test]
async fn a_checked_translation_reads_back_as_written() {
    support::a_checked_translation_reads_back_as_written(
        MemoryDatabase::new().repository::<Piece>(),
    )
    .await;
}
