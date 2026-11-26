//! Codegen assertions for the YSQL `#[derive(Model)]` output; no database required.

use fundamental_core::model::{Model, SqlModel};
use fundamental_db::SchemaName;
use fundamental_macros::Model;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql)]
struct Widget {
    #[model(id)]
    id: uuid::Uuid,
    #[model(indexed)]
    sku: String,
    name: Option<String>,
}

#[test]
fn base_model_metadata() {
    assert_eq!(<Widget as Model>::TYPE_NAME, "Widget");
}

#[test]
fn sql_model_surface() {
    assert_eq!(<Widget as SqlModel>::TABLE, "widgets");
    assert_eq!(<Widget as SqlModel>::PRIMARY_KEY, "id");
    assert_eq!(<Widget as SqlModel>::DOMAIN, "default");
    // The schema comes from the pool, not the model.
    assert_eq!(
        SchemaName::default().qualify(<Widget as SqlModel>::TABLE),
        "public.widgets"
    );
}

#[test]
fn typed_primary_key_accessor() {
    let id = uuid::Uuid::nil();
    let w = Widget {
        id,
        sku: "abc".into(),
        name: None,
    };
    assert_eq!(*w.id(), id);
}

#[test]
fn typed_column_constants() {
    assert_eq!(Widget::ID, "id");
    assert_eq!(Widget::SKU, "sku");
    assert_eq!(Widget::NAME, "name");
}

/// `PRIMARY_KEY` must resolve `#[model(column = "…")]`, not the field's ident.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql)]
struct RenamedKey {
    #[model(id, column = "widget_id")]
    id: uuid::Uuid,
    name: String,
}

#[test]
fn a_renamed_key_column_is_named_by_its_column_attribute_not_its_field() {
    assert_eq!(
        <RenamedKey as SqlModel>::PRIMARY_KEY,
        "widget_id",
        "PRIMARY_KEY reported the field ident, naming a column the table does not have"
    );
    assert_eq!(
        <RenamedKey as SqlModel>::KEY_COLUMNS,
        ["widget_id"],
        "the key column list and PRIMARY_KEY must agree"
    );
}
