//! Crate-internal SQL statement builders for YSQL row operations.

mod delete;
mod insert;
mod update;
mod upsert;

pub(crate) use delete::DeleteBuilder;
pub(crate) use insert::InsertBuilder;
pub(crate) use update::UpdateBuilder;
pub(crate) use upsert::UpsertBuilder;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BindValues, ToRow};

    struct TestModel;
    impl ToRow for TestModel {
        fn columns() -> &'static [&'static str] {
            &["id", "name", "email"]
        }
        fn bind_to<'q, Q: BindValues<'q>>(&'q self, query: Q) -> Q {
            query
        }
        fn bind_without_pk<'q, Q: BindValues<'q>>(&'q self, query: Q) -> Q {
            query
        }
        fn bind_key<'q, Q: BindValues<'q>>(&'q self, query: Q) -> Q {
            query
        }
    }

    #[test]
    fn test_insert_builder() {
        let sql = InsertBuilder::new("users")
            .columns_from::<TestModel>()
            .returning("*")
            .build();

        assert_eq!(
            sql,
            "INSERT INTO users (id, name, email) VALUES ($1, $2, $3) RETURNING *"
        );
    }

    #[test]
    fn test_update_builder() {
        let sql = UpdateBuilder::new("users")
            .columns_from::<TestModel>()
            .where_pk(&["id"])
            .returning("*")
            .build();

        assert_eq!(
            sql,
            "UPDATE users SET name = $1, email = $2 WHERE id = $3 RETURNING *"
        );
    }

    #[test]
    fn test_delete_builder() {
        let sql = DeleteBuilder::new("users").where_pk(&["id"]).build();

        assert_eq!(sql, "DELETE FROM users WHERE id = $1");
    }

    #[test]
    fn a_composite_key_is_matched_on_every_column() {
        let update = UpdateBuilder::new("holder_accounts")
            .where_pk(&["zone", "holder_account_id"])
            .build();
        assert!(
            update.contains("WHERE zone = $1 AND holder_account_id = $2"),
            "{update}"
        );

        let delete = DeleteBuilder::new("holder_accounts")
            .where_pk(&["zone", "holder_account_id"])
            .build();
        assert!(
            delete.contains("WHERE zone = $1 AND holder_account_id = $2"),
            "{delete}"
        );
    }

    #[test]
    fn test_upsert_builder() {
        let sql = UpsertBuilder::new("users")
            .columns_from::<TestModel>()
            .on_conflict(&["id"])
            .returning("*")
            .build::<TestModel>();

        assert_eq!(
            sql,
            "INSERT INTO users (id, name, email) VALUES ($1, $2, $3) ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, email = EXCLUDED.email RETURNING *"
        );
    }

    #[test]
    fn test_upsert_builder_do_nothing() {
        let sql = UpsertBuilder::new("users")
            .columns_from::<TestModel>()
            .on_conflict(&["email"])
            .do_nothing()
            .returning("*")
            .build::<TestModel>();

        assert_eq!(
            sql,
            "INSERT INTO users (id, name, email) VALUES ($1, $2, $3) ON CONFLICT (email) DO NOTHING RETURNING *"
        );
    }

    #[test]
    fn test_upsert_builder_guarded() {
        let sql = UpsertBuilder::new("heads")
            .columns_from::<TestModel>()
            .on_conflict(&["id"])
            .guard("excluded.name >= heads.name")
            .returning("*")
            .build::<TestModel>();

        assert_eq!(
            sql,
            "INSERT INTO heads (id, name, email) VALUES ($1, $2, $3) ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, email = EXCLUDED.email WHERE excluded.name >= heads.name RETURNING *"
        );
    }

    #[test]
    fn test_upsert_builder_composite_conflict_target() {
        let sql = UpsertBuilder::new("users")
            .columns_from::<TestModel>()
            .on_conflict(&["id", "email"])
            .do_nothing()
            .returning("*")
            .build::<TestModel>();

        assert_eq!(
            sql,
            "INSERT INTO users (id, name, email) VALUES ($1, $2, $3) ON CONFLICT (id, email) DO NOTHING RETURNING *"
        );
    }

    #[test]
    fn test_upsert_builder_guard_ignored_when_do_nothing() {
        let sql = UpsertBuilder::new("heads")
            .columns_from::<TestModel>()
            .on_conflict(&["id"])
            .do_nothing()
            .guard("excluded.name >= heads.name")
            .build::<TestModel>();

        assert_eq!(
            sql,
            "INSERT INTO heads (id, name, email) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING"
        );
    }

    #[test]
    fn test_columns_without_pk() {
        let cols = TestModel::columns_without_pk();
        assert_eq!(cols, &["name", "email"]);
    }

    #[test]
    fn test_column_count() {
        assert_eq!(TestModel::column_count(), 3);
    }
}
