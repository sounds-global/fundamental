//! Rendering of a generated slice source file.

use crate::engine::Engine;

/// The `domain/src/sync/<snake>.rs` source: a syncable base model and a `#[slice]` resolver.
///
/// YSQL keys on one id; YCQL on `((owner_id), id)`, the owner being the outbox's partition key.
pub(super) fn slice_source(snake: &str, pascal: &str, engine: &Engine) -> String {
    // Named apart from the `{pascal}` marker `#[slice]` emits.
    let (engine_attr, model_body) = match engine {
        Engine::CqlDb => (
            "cql",
            "\x20   /// The owning principal — the partition key the outbox fans out by\n\
             \x20   /// and the column this slice scopes rows by.\n\
             \x20   #[model(partition_key, owner)]\n\
             \x20   pub owner_id: String,\n\
             \x20   /// Primary key (a clustering key under the owner partition).\n\
             \x20   #[model(id, clustering_key)]\n\
             \x20   pub id: Uuid,\n",
        ),
        Engine::SqlDb | Engine::Other(_) => (
            "sql",
            "\x20   /// Primary key.\n\
             \x20   #[model(id)]\n\
             \x20   pub id: Uuid,\n\
             \x20   /// The owning principal — the column this slice scopes rows by.\n\
             \x20   #[model(owner)]\n\
             \x20   pub owner_id: String,\n",
        ),
    };
    format!(
        "//! The `{snake}` slice and its `{pascal}Row` base model.\n\n\
         use fundamental::Model;\n\
         use fundamental::slice;\n\
         use fundamental::sync::NoParams;\n\
         use uuid::Uuid;\n\n\
         /// The `{pascal}Row` base table: a syncable model mirrored on each device.\n\
         #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, Model)]\n\
         #[model(offline, syncable, {engine_attr}, table = \"{snake}\")]\n\
         pub struct {pascal}Row {{\n\
         {model_body}\
         }}\n\n\
         /// The `{snake}` slice: the caller's own `{pascal}Row` rows.\n\
         #[slice({pascal}Row, \"{snake}\")]\n\
         async fn {snake}(ctx: SliceContext, _params: NoParams) -> Result<Filter, SliceError> {{\n\
         \x20   Ok(Filter::eq(\"owner_id\", ctx.user_id().as_str()))\n\
         }}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_slice_keys_on_an_id_and_marks_the_owner() {
        let src = slice_source("mine", "Mine", &Engine::SqlDb);
        assert!(src.contains("#[slice(MineRow, \"mine\")]"));
        assert!(src.contains("#[model(offline, syncable, sql, table = \"mine\")]"));
        assert!(src.contains("#[model(id)]"));
        assert!(
            src.contains("#[model(owner)]\n    pub owner_id: String,"),
            "{src}"
        );
        assert!(src.contains("async fn mine(ctx: SliceContext"));
        assert!(src.contains("pub struct MineRow"));
        assert!(!src.contains("cfg"));
        assert!(!src.contains("feature ="));
    }

    #[test]
    fn cql_slice_keys_on_the_owner_partition_and_marks_the_owner() {
        let src = slice_source("mine", "Mine", &Engine::CqlDb);
        assert!(src.contains("#[model(offline, syncable, cql, table = \"mine\")]"));
        assert!(src.contains("#[model(partition_key, owner)]"), "{src}");
        assert!(src.contains("#[model(id, clustering_key)]"), "{src}");
        assert!(!src.contains("cfg"));
    }

    #[test]
    fn an_unknown_engine_falls_back_to_the_sql_shape() {
        let src = slice_source("mine", "Mine", &Engine::Other("duckdb".into()));
        assert!(src.contains("#[model(offline, syncable, sql, table = \"mine\")]"));
    }
}
