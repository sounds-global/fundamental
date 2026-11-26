//! Rendering of a generated model source file.

use super::Field;
use super::ModelName;
use crate::engine::Engine;

/// The `src/models/<snake>.rs` source: the id is the YSQL primary key or the YCQL partition key.
pub(super) fn model_source(name: &ModelName, fields: &[Field], engine: &Engine) -> String {
    let type_name = name.type_name();
    let snake = name.snake();
    let (db_attr, id_attr) = match engine {
        Engine::CqlDb => ("cql", "partition_key"),
        Engine::SqlDb | Engine::Other(_) => ("sql", "id"),
    };

    let mut out = String::new();
    out.push_str(&format!("//! The `{type_name}` model.\n\n"));
    out.push_str("use fundamental::Model;\n");
    out.push_str("use uuid::Uuid;\n\n");
    out.push_str(&format!("/// The `{type_name}` model.\n"));
    out.push_str(
        "#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, Model)]\n",
    );
    out.push_str(&format!("#[model({db_attr}, table = \"{snake}\")]\n"));
    out.push_str(&format!("pub struct {type_name} {{\n"));
    out.push_str("    /// Primary key.\n");
    out.push_str(&format!("    #[model({id_attr})]\n"));
    out.push_str("    pub id: Uuid,\n");
    for field in fields {
        out.push_str(&format!(
            "    /// The `{name}` field.\n    pub {name}: {ty},\n",
            name = field.name(),
            ty = field.rust_type()
        ));
    }
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> Vec<Field> {
        vec![
            Field::parse("title:String").unwrap(),
            Field::parse("views:i64").unwrap(),
        ]
    }

    #[test]
    fn sql_model_uses_id_and_table() {
        let name = ModelName::parse("Post").unwrap();
        let src = model_source(&name, &fields(), &Engine::SqlDb);
        assert!(src.contains("use fundamental::Model;"), "{src}");
        assert!(src.contains("use uuid::Uuid;"), "{src}");
        assert!(
            src.contains(
                "#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, Model)]"
            ),
            "{src}"
        );
        assert!(src.contains("#[model(sql, table = \"post\")]"), "{src}");
        assert!(!src.contains("cfg_attr"), "{src}");
        assert!(!src.contains("feature = \"server\""), "{src}");
        assert!(src.contains("#[model(id)]"), "{src}");
        assert!(src.contains("pub id: Uuid,"), "{src}");
        assert!(src.contains("pub title: String,"), "{src}");
        assert!(src.contains("pub views: i64,"), "{src}");
        assert!(src.contains("pub struct Post {"), "{src}");
    }

    #[test]
    fn cql_model_uses_a_partition_key() {
        let name = ModelName::parse("Event").unwrap();
        let src = model_source(&name, &[], &Engine::CqlDb);
        assert!(src.contains("#[model(cql, table = \"event\")]"), "{src}");
        assert!(!src.contains("cfg_attr"), "{src}");
        assert!(src.contains("#[model(partition_key)]"), "{src}");
        assert!(!src.contains("#[model(id)]"), "{src}");
    }

    #[test]
    fn no_fields_still_emits_the_id() {
        let name = ModelName::parse("Tag").unwrap();
        let src = model_source(&name, &[], &Engine::SqlDb);
        assert!(src.contains("pub id: Uuid,"), "{src}");
    }
}
