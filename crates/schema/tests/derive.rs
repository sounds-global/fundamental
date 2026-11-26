//! End-to-end coverage of `#[derive(Schema)]` across the shapes it supports.
#![allow(dead_code)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use fundamental_schema::{Schema, SchemaNode};
use serde_json::json;

fn schema_json<T: Schema>() -> serde_json::Value {
    serde_json::to_value(T::schema()).expect("a schema serializes")
}

#[derive(Schema)]
struct CreateUser {
    name: String,
    age: u8,
    nickname: Option<String>,
}

#[test]
fn named_struct_marks_non_option_fields_required() {
    assert_eq!(
        schema_json::<CreateUser>(),
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "age": { "type": "integer", "minimum": 0, "maximum": 255 },
                "nickname": { "anyOf": [{ "type": "string" }, { "type": "null" }] }
            },
            "required": ["name", "age"]
        })
    );
}

#[derive(Schema)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
struct Renamed {
    user_id: u32,
    #[serde(rename = "displayName")]
    name: String,
    #[serde(skip)]
    #[allow(dead_code)]
    internal: String,
}

#[test]
fn rename_all_and_field_rename_and_skip_are_honored() {
    assert_eq!(
        schema_json::<Renamed>(),
        json!({
            "type": "object",
            "properties": {
                "userId": { "type": "integer", "minimum": 0, "maximum": 4294967295u32 },
                "displayName": { "type": "string" }
            },
            "required": ["userId", "displayName"]
        })
    );
}

#[derive(Schema)]
struct UserId(u64);

#[test]
fn newtype_is_transparent() {
    assert_eq!(
        schema_json::<UserId>(),
        json!({ "type": "integer", "minimum": 0 })
    );
}

#[derive(Schema)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
enum Color {
    Red,
    ForestGreen,
    #[serde(rename = "azure")]
    Blue,
}

#[test]
fn fieldless_enum_is_a_string_enum() {
    assert_eq!(
        schema_json::<Color>(),
        json!({ "type": "string", "enum": ["red", "forest_green", "azure"] })
    );
}

/// A kind a person never enters: worked out, so no input holds it.
#[derive(Schema)]
#[serde(rename_all = "snake_case")]
#[expect(
    dead_code,
    reason = "only its schema is asserted; the derive reads the serde attributes"
)]
enum Kind {
    Received,
    Spent,
    #[serde(skip)]
    Available,
}

#[test]
fn a_variant_serde_skips_is_no_value_the_enum_takes() {
    assert_eq!(
        schema_json::<Kind>(),
        json!({ "type": "string", "enum": ["received", "spent"] })
    );
}

#[derive(Schema)]
struct Address {
    city: String,
}

#[derive(Schema)]
struct Profile {
    owner: CreateUser,
    address: Address,
    tags: Vec<String>,
}

#[test]
fn nested_structs_compose() {
    let value = schema_json::<Profile>();
    assert_eq!(value["properties"]["owner"]["type"], "object");
    assert_eq!(
        value["properties"]["address"]["properties"]["city"]["type"],
        "string"
    );
    assert_eq!(value["properties"]["tags"]["type"], "array");
    assert_eq!(value["properties"]["tags"]["items"]["type"], "string");
}

#[derive(Schema)]
struct Wrapper<T> {
    value: T,
    count: u32,
}

#[test]
fn generic_struct_bounds_each_parameter() {
    let value = schema_json::<Wrapper<bool>>();
    assert_eq!(value["properties"]["value"]["type"], "boolean");
    assert_eq!(value["properties"]["count"]["type"], "integer");
}

#[test]
fn a_day_is_a_string_in_the_date_format() {
    assert_eq!(
        <chrono::NaiveDate as Schema>::schema(),
        SchemaNode::String {
            format: Some("date"),
            pattern: None,
        }
    );
}

#[test]
fn a_moment_is_a_string_in_the_date_time_format() {
    assert_eq!(
        <chrono::DateTime<chrono::Utc> as Schema>::schema(),
        SchemaNode::String {
            format: Some("date-time"),
            pattern: None,
        }
    );
}

#[test]
fn an_amount_of_money_is_a_string_in_the_money_format() {
    assert_eq!(
        <fundamental_core::money::Money as Schema>::schema(),
        SchemaNode::String {
            format: Some("money"),
            pattern: None,
        }
    );
}
