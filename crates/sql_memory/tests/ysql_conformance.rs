//! YugabyteDB's repositories pass the conformance suite; requires Docker.

mod support;

use fundamental_core::model::{SqlDb, SqlModel};
use fundamental_db::migration::{ModelSchema, render};
use fundamental_sql::{FromRow, SqlRepository, ToRow};
use fundamental_test_containers::sql::SqlSchema;
use support::{Draft, Entry, Note, Piece, Slugged};

/// A schema holding `M`'s tables as the generated migrations make them, and `M`'s repository on it.
async fn on_ysql<M>() -> (SqlSchema, SqlRepository<M>)
where
    M: ModelSchema<SqlDb> + SqlModel + FromRow + ToRow + Send + Sync,
{
    let schema = SqlSchema::mint().await.expect("a test schema");
    let own = M::table_schema();
    let companion = M::translation_schema();
    for statement in render::create_all(&own)
        .into_iter()
        .chain(companion.iter().flat_map(render::create_all))
    {
        schema
            .pool()
            .execute_ddl(&statement)
            .await
            .expect("the table exists");
    }
    let repository = schema.pool().repository::<M>();
    (schema, repository)
}

#[tokio::test]
async fn a_row_inserted_is_found_by_its_id() {
    let (_schema, notes) = on_ysql::<Note>().await;
    support::a_row_inserted_is_found_by_its_id(notes).await;
}

#[tokio::test]
async fn a_second_row_of_one_key_is_refused_as_its_primary_key() {
    let (_schema, notes) = on_ysql::<Note>().await;
    support::a_second_row_of_one_key_is_refused_as_its_primary_key(notes).await;
}

#[tokio::test]
async fn an_update_replaces_the_row_its_key_names() {
    let (_schema, notes) = on_ysql::<Note>().await;
    support::an_update_replaces_the_row_its_key_names(notes).await;
}

#[tokio::test]
async fn an_upsert_writes_a_row_or_replaces_the_one_its_key_names() {
    let (_schema, notes) = on_ysql::<Note>().await;
    support::an_upsert_writes_a_row_or_replaces_the_one_its_key_names(notes).await;
}

#[tokio::test]
async fn a_row_deleted_is_gone_and_said_once() {
    let (_schema, notes) = on_ysql::<Note>().await;
    support::a_row_deleted_is_gone_and_said_once(notes).await;
}

#[tokio::test]
async fn a_query_keeps_the_rows_a_comparison_matches() {
    let (_schema, notes) = on_ysql::<Note>().await;
    support::a_query_keeps_the_rows_a_comparison_matches(notes).await;
}

#[tokio::test]
async fn a_query_keeps_the_rows_a_set_a_null_or_a_pattern_matches() {
    let (_schema, notes) = on_ysql::<Note>().await;
    support::a_query_keeps_the_rows_a_set_a_null_or_a_pattern_matches(notes).await;
}

#[tokio::test]
async fn a_query_keeps_the_rows_a_combination_matches() {
    let (_schema, notes) = on_ysql::<Note>().await;
    support::a_query_keeps_the_rows_a_combination_matches(notes).await;
}

#[tokio::test]
async fn a_query_reads_its_rows_in_order_a_page_at_a_time() {
    let (_schema, notes) = on_ysql::<Note>().await;
    support::a_query_reads_its_rows_in_order_a_page_at_a_time(notes).await;
}

#[tokio::test]
async fn a_null_sorts_last_going_up_and_first_going_down() {
    let (_schema, notes) = on_ysql::<Note>().await;
    support::a_null_sorts_last_going_up_and_first_going_down(notes).await;
}

#[tokio::test]
async fn a_count_counts_every_row_its_filters_keep() {
    let (_schema, notes) = on_ysql::<Note>().await;
    support::a_count_counts_every_row_its_filters_keep(notes).await;
}

#[tokio::test]
async fn a_row_deleted_softly_is_read_only_when_asked_for() {
    let (_schema, drafts) = on_ysql::<Draft>().await;
    support::a_row_deleted_softly_is_read_only_when_asked_for(drafts).await;
}

#[tokio::test]
async fn a_row_deleted_softly_is_restored_or_purged() {
    let (_schema, drafts) = on_ysql::<Draft>().await;
    support::a_row_deleted_softly_is_restored_or_purged(drafts).await;
}

#[tokio::test]
async fn a_delete_by_filter_removes_the_rows_it_keeps() {
    let (_schema, notes) = on_ysql::<Note>().await;
    support::a_delete_by_filter_removes_the_rows_it_keeps(notes).await;
}

#[tokio::test]
async fn a_row_is_one_organisations_by_its_leading_key() {
    let (_schema, entries) = on_ysql::<Entry>().await;
    support::a_row_is_one_organisations_by_its_leading_key(entries).await;
}

#[tokio::test]
async fn a_row_reads_back_with_every_value_it_was_written_with() {
    let (_schema, pieces) = on_ysql::<Piece>().await;
    support::a_row_reads_back_with_every_value_it_was_written_with(pieces).await;
}

#[tokio::test]
async fn a_column_the_table_lacks_is_refused() {
    let (_schema, notes) = on_ysql::<Note>().await;
    support::a_column_the_table_lacks_is_refused(notes).await;
}

#[tokio::test]
async fn a_database_hands_out_each_models_repository() {
    let (schema, _) = on_ysql::<Note>().await;
    let entries = <Entry as ModelSchema<SqlDb>>::table_schema();
    for statement in render::create_all(&entries) {
        schema
            .pool()
            .execute_ddl(&statement)
            .await
            .expect("the table exists");
    }
    support::a_database_hands_out_each_models_repository(schema.pool().clone()).await;
}

#[tokio::test]
async fn a_unique_index_refuses_a_second_row_of_its_columns() {
    let (_schema, slugs) = on_ysql::<Slugged>().await;
    support::a_unique_index_refuses_a_second_row_of_its_columns(slugs).await;
}

#[tokio::test]
async fn a_unique_index_holds_the_rows_its_condition_keeps_and_no_null() {
    let (_schema, slugs) = on_ysql::<Slugged>().await;
    support::a_unique_index_holds_the_rows_its_condition_keeps_and_no_null(slugs).await;
}

#[tokio::test]
async fn a_filter_its_column_cannot_take_is_refused() {
    let (_schema, notes) = on_ysql::<Note>().await;
    support::a_filter_its_column_cannot_take_is_refused(notes).await;
}

#[tokio::test]
async fn a_flag_a_day_and_a_moment_take_only_their_own_values() {
    let (_schema, pieces) = on_ysql::<Piece>().await;
    support::a_flag_a_day_and_a_moment_take_only_their_own_values(pieces).await;
}

#[tokio::test]
async fn a_moment_reads_back_to_the_microsecond_it_is_kept_at() {
    let (_schema, pieces) = on_ysql::<Piece>().await;
    support::a_moment_reads_back_to_the_microsecond_it_is_kept_at(pieces).await;
}

#[tokio::test]
async fn a_checked_translation_reads_back_as_written() {
    let (_schema, pieces) = on_ysql::<Piece>().await;
    support::a_checked_translation_reads_back_as_written(pieces).await;
}
