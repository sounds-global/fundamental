//! A YSQL model may be keyed by more than one column; its `Id` is deliberately not `sqlx::Encode`.

use fundamental_core::model::{
    CompositeKeyModel, HasLeadingKey, IndexDef, LeadingKeyAddressed, Model, NoLeadingKey,
    Satisfied, SingleKeyModel, SqlDb, SqlModel,
};
use fundamental_db::{
    FilterCondition, FilterValue, QueryBuilder, RepositoryAggregate,
    RepositoryTransactionAggregate, RepositoryTransactionQuery,
};
use fundamental_macros::Model;
use fundamental_sql::{SqlRepository, SqlTransaction, ToRow};

#[derive(Model)]
#[model(sql)]
struct Session {
    #[model(id)]
    session_id: String,
    device: String,
}

#[derive(Model)]
#[model(sql)]
struct HolderAccount {
    #[model(id)]
    zone: String,
    #[model(id)]
    holder_account_id: String,
    created_at: String,
}

#[derive(Model)]
#[model(sql)]
struct GrantScope {
    #[model(id)]
    zone: String,
    #[model(id)]
    grant_id: String,
    #[model(id)]
    attribute: String,
    allowed: bool,
}

#[derive(Model)]
#[model(
    sql,
    index(name = "memberships_by_account", columns = ["staff_account_id", "organisation_id"])
)]
struct Membership {
    #[model(id)]
    organisation_id: String,
    #[model(id)]
    staff_account_id: String,
    joined_at: String,
}

#[test]
fn a_single_key_model_is_unchanged() {
    assert_eq!(<Session as SqlModel>::PRIMARY_KEY, "session_id");
    assert_eq!(
        <Session as SqlModel>::KEY_COLUMNS,
        &["session_id"],
        "a one-column key must report itself identically through both constants, or every \
         existing model changes meaning"
    );
    assert_eq!(<Session as Model>::TYPE_NAME, "Session");
}

#[test]
fn a_composite_key_reports_every_column_in_key_order() {
    assert_eq!(
        <HolderAccount as SqlModel>::KEY_COLUMNS,
        &["zone", "holder_account_id"]
    );
    assert_eq!(
        <GrantScope as SqlModel>::KEY_COLUMNS,
        &["zone", "grant_id", "attribute"]
    );
}

/// Declaration order is key order, and key order decides placement.
#[test]
fn the_leading_key_column_is_the_one_declared_first() {
    assert_eq!(<HolderAccount as SqlModel>::PRIMARY_KEY, "zone");
    assert_eq!(<GrantScope as SqlModel>::PRIMARY_KEY, "zone");
    assert_eq!(
        <HolderAccount as SqlModel>::KEY_COLUMNS[0],
        <HolderAccount as SqlModel>::PRIMARY_KEY,
        "PRIMARY_KEY is the leading column of KEY_COLUMNS, not a second opinion"
    );
}

#[test]
fn the_ddl_emits_one_composite_primary_key() {
    let sql = HolderAccount::create_table_sql();
    assert!(
        sql.contains("PRIMARY KEY (zone, holder_account_id)"),
        "expected a composite primary key, got:\n{sql}"
    );
    assert_eq!(
        sql.matches("PRIMARY KEY").count(),
        1,
        "two PRIMARY KEY clauses is not a table YSQL will accept:\n{sql}"
    );

    let three = GrantScope::create_table_sql();
    assert!(
        three.contains("PRIMARY KEY (zone, grant_id, attribute)"),
        "{three}"
    );
}

#[test]
fn a_single_key_models_ddl_is_untouched() {
    let sql = Session::create_table_sql();
    assert!(sql.contains("PRIMARY KEY (session_id)"), "{sql}");
}

#[test]
fn every_key_column_is_not_null() {
    let sql = HolderAccount::create_table_sql();
    for column in ["zone", "holder_account_id"] {
        let line = sql
            .lines()
            .find(|line| line.trim_start().starts_with(column))
            .unwrap_or_else(|| panic!("{column} is missing from:\n{sql}"));
        assert!(line.contains("NOT NULL"), "{column} is nullable: {line}");
    }
}

/// Every key column is excluded, or an UPDATE would set and match the same column.
#[test]
fn the_payload_excludes_every_key_column() {
    assert_eq!(
        <HolderAccount as ToRow>::columns_without_pk(),
        &["created_at"]
    );
    assert_eq!(<GrantScope as ToRow>::columns_without_pk(), &["allowed"]);
    assert_eq!(<Session as ToRow>::columns_without_pk(), &["device"]);
}

#[test]
fn the_key_and_the_payload_partition_the_columns() {
    for (all, payload, key) in [
        (
            <HolderAccount as ToRow>::columns(),
            <HolderAccount as ToRow>::columns_without_pk(),
            <HolderAccount as SqlModel>::KEY_COLUMNS,
        ),
        (
            <GrantScope as ToRow>::columns(),
            <GrantScope as ToRow>::columns_without_pk(),
            <GrantScope as SqlModel>::KEY_COLUMNS,
        ),
        (
            <Session as ToRow>::columns(),
            <Session as ToRow>::columns_without_pk(),
            <Session as SqlModel>::KEY_COLUMNS,
        ),
    ] {
        assert_eq!(
            all.len(),
            payload.len() + key.len(),
            "columns {all:?} do not partition into key {key:?} and payload {payload:?}"
        );
        for column in key {
            assert!(
                !payload.contains(column),
                "{column} is both a key column and a payload column"
            );
            assert!(
                all.contains(column),
                "{column} is a key column but not a column"
            );
        }
    }
}

#[test]
fn only_a_single_key_model_is_marked_as_one() {
    fn assert_single_key<M: SingleKeyModel>() {}
    assert_single_key::<Session>();
    // `assert_single_key::<HolderAccount>()` does not compile.
}

#[test]
fn a_composite_key_model_still_declares_every_column() {
    assert_eq!(
        <HolderAccount as ToRow>::columns(),
        &["zone", "holder_account_id", "created_at"]
    );
}

#[test]
fn a_composite_key_model_carries_its_key_values_as_a_tuple() {
    fn assert_composite<M: CompositeKeyModel>() {}
    assert_composite::<HolderAccount>();
    assert_composite::<GrantScope>();
    // `assert_composite::<Session>()` does not compile.

    let account = HolderAccount {
        zone: "eu".into(),
        holder_account_id: "acct-1".into(),
        created_at: "now".into(),
    };
    let key: (String, String) = account.key_values();
    assert_eq!(
        key,
        ("eu".to_string(), "acct-1".to_string()),
        "the tuple is the key columns' values in key order, so position 0 is the placement axis"
    );

    let scope = GrantScope {
        zone: "eu".into(),
        grant_id: "g-1".into(),
        attribute: "email".into(),
        allowed: true,
    };
    let key: (String, String, String) = scope.key_values();
    assert_eq!(key.0, "eu", "three columns behave exactly as two do");
}

#[test]
fn the_key_tuples_width_is_the_number_of_key_columns() {
    fn assert_arity<M: CompositeKeyModel>(model: &M, expected: usize)
    where
        M::KeyValues: std::fmt::Debug,
    {
        assert_eq!(
            M::KEY_COLUMNS.len(),
            expected,
            "{} declares {:?}",
            M::TYPE_NAME,
            M::KEY_COLUMNS
        );
        let _ = model.key_values();
    }

    assert_arity(
        &HolderAccount {
            zone: "eu".into(),
            holder_account_id: "a".into(),
            created_at: "n".into(),
        },
        2,
    );
    assert_arity(
        &GrantScope {
            zone: "eu".into(),
            grant_id: "g".into(),
            attribute: "email".into(),
            allowed: false,
        },
        3,
    );
}

/// A test rather than a comment, so deleting the witness fails as loudly as breaking it.
#[test]
fn the_leading_key_witness_holds_exactly_where_it_should() {
    fn assert_addressable<M, D, State>()
    where
        LeadingKeyAddressed<M, D, State>: Satisfied,
    {
    }

    assert_addressable::<Session, SqlDb, NoLeadingKey>();

    assert_addressable::<HolderAccount, SqlDb, HasLeadingKey>();
    assert_addressable::<GrantScope, SqlDb, HasLeadingKey>();

    // Unpinned, `assert_addressable::<HolderAccount, SqlDb, NoLeadingKey>()` does not compile.
}

/// The pinned state is reached only through `pin_key`, which reads the model's own `KEY_COLUMNS`.
#[test]
fn pinning_the_leading_key_adds_a_predicate_on_the_models_own_column() {
    let pinned = QueryBuilder::<HolderAccount, SqlDb>::new().pin_key("eu");
    let filters = pinned.get_filters();
    assert_eq!(filters.len(), 1, "pin_key adds exactly one predicate");
    let FilterCondition::Eq { column, value } = &filters[0].condition else {
        panic!("pin_key must pin with an equality, the only form an index prefix can serve");
    };
    assert_eq!(
        column,
        HolderAccount::KEY_COLUMNS[0],
        "the pinned column is the model's leading key column, never one the caller named"
    );
    assert!(
        matches!(value, FilterValue::String(pinned) if pinned == "eu"),
        "the pinned value reaches the predicate unchanged, got {value:?}"
    );
}

#[test]
fn a_composite_key_model_can_query_and_aggregate_inside_a_transaction() {
    fn assert_tx_query<M>()
    where
        SqlTransaction: RepositoryTransactionQuery<M, SqlDb>,
        M: fundamental_core::model::BoundTo<SqlDb>
            + SqlModel
            + Model
            + fundamental_sql::FromRow
            + ToRow
            + Send
            + Sync,
    {
    }
    fn assert_tx_aggregate<M>()
    where
        SqlTransaction: RepositoryTransactionAggregate<M, SqlDb>,
        M: fundamental_core::model::BoundTo<SqlDb>
            + SqlModel
            + Model
            + fundamental_sql::FromRow
            + ToRow
            + Send
            + Sync,
    {
    }

    assert_tx_query::<HolderAccount>();
    assert_tx_query::<GrantScope>();
    assert_tx_query::<Session>();
    assert_tx_aggregate::<HolderAccount>();
    assert_tx_aggregate::<Session>();
}

#[test]
fn every_key_shape_can_count_a_window() {
    fn assert_counts<M>()
    where
        SqlTransaction: fundamental_db::RepositoryTransactionCount<M, SqlDb>,
        M: fundamental_core::model::BoundTo<SqlDb> + Send + Sync,
    {
    }
    assert_counts::<HolderAccount>();
    assert_counts::<GrantScope>();
    assert_counts::<Session>();
}

#[test]
fn a_composite_key_model_can_aggregate_on_the_pool() {
    fn assert_aggregate<M>()
    where
        SqlRepository<M>: RepositoryAggregate<M, SqlDb>,
        M: fundamental_core::model::BoundTo<SqlDb>
            + SqlModel
            + Model
            + fundamental_sql::FromRow
            + ToRow
            + Send
            + Sync,
    {
    }
    assert_aggregate::<HolderAccount>();
    assert_aggregate::<Session>();
}

#[test]
fn a_composite_key_model_has_a_keyed_read_and_delete_on_the_repository() {
    fn assert_keyed_repo<M>()
    where
        M: fundamental_core::model::BoundTo<SqlDb>
            + SqlModel
            + CompositeKeyModel
            + Model
            + fundamental_sql::FromRow
            + ToRow
            + Send
            + Sync,
        for<'k> M::KeyValues: fundamental_sql::BindKey<'k>,
    {
        let _read = SqlRepository::<M>::find_by_key;
        let _delete = SqlRepository::<M>::delete_by_key;
    }
    assert_keyed_repo::<HolderAccount>();
    assert_keyed_repo::<GrantScope>();
    // `assert_keyed_repo::<Session>()` does not compile.
}

#[test]
fn a_declared_index_pins_its_leading_column() {
    let pinned = QueryBuilder::<Membership, SqlDb>::new()
        .pin_index(Membership::MEMBERSHIPS_BY_ACCOUNT, "acct-1");
    let filters = pinned.get_filters();
    assert_eq!(filters.len(), 1, "pin_index adds exactly one predicate");
    let FilterCondition::Eq { column, value } = &filters[0].condition else {
        panic!("pin_index must pin with an equality, the only form an index prefix can serve");
    };
    assert_eq!(column, "staff_account_id");
    assert_ne!(
        column,
        Membership::KEY_COLUMNS[0],
        "an index read is not a primary-key read under another name"
    );
    assert!(
        matches!(value, FilterValue::String(pinned) if pinned == "acct-1"),
        "the pinned value reaches the predicate unchanged, got {value:?}"
    );
}

#[test]
fn a_model_lists_the_indexes_it_declares() {
    assert_eq!(
        <Membership as SqlModel>::INDEXES,
        [IndexDef::new(
            "memberships_by_account",
            &["staff_account_id", "organisation_id"]
        )]
    );
    assert_eq!(
        Membership::MEMBERSHIPS_BY_ACCOUNT.name(),
        "memberships_by_account"
    );
    assert!(<HolderAccount as SqlModel>::INDEXES.is_empty());
    assert!(<Session as SqlModel>::INDEXES.is_empty());
}
