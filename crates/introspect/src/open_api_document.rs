use serde::Serialize;
use serde_json::{Map, Value, json};

use fundamental_server::ActionDescriptor;

/// An [OpenAPI 3.1] description of an HTTP action surface.
///
/// [OpenAPI 3.1]: https://spec.openapis.org/oas/v3.1.0
#[derive(Debug, Clone)]
pub struct OpenApiDocument {
    title: String,
    version: String,
    actions: Vec<ActionDescriptor>,
}

impl OpenApiDocument {
    /// Builds a document from a title, an API version, and the action descriptors.
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        version: impl Into<String>,
        actions: Vec<ActionDescriptor>,
    ) -> Self {
        Self {
            title: title.into(),
            version: version.into(),
            actions,
        }
    }

    /// Renders the document as a JSON value.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut paths: Map<String, Value> = Map::new();
        let mut any_auth = false;

        for action in &self.actions {
            any_auth |= action.requires_auth();
            let operation = self.operation(action);
            let entry = paths
                .entry(action.path.to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            if let Value::Object(methods) = entry {
                methods.insert(action.method.to_ascii_lowercase(), operation);
            }
        }

        let mut document = json!({
            "openapi": "3.1.0",
            "info": { "title": self.title, "version": self.version },
            "paths": Value::Object(paths),
        });

        if any_auth {
            document["components"] = json!({
                "securitySchemes": {
                    "bearerAuth": { "type": "http", "scheme": "bearer" }
                }
            });
        }

        document
    }

    fn operation(&self, action: &ActionDescriptor) -> Value {
        let mut operation = Map::new();
        if !action.doc.is_empty() {
            operation.insert("summary".into(), Value::String(action.doc.to_string()));
        }

        if action.has_request_body() {
            operation.insert(
                "requestBody".into(),
                json!({
                    "required": true,
                    "content": { "application/json": { "schema": action.input } }
                }),
            );
        }

        operation.insert(
            "responses".into(),
            json!({
                "200": {
                    "description": "OK",
                    "content": { "application/json": { "schema": action.output } }
                }
            }),
        );

        if action.requires_auth() {
            operation.insert("security".into(), json!([{ "bearerAuth": [] }]));
        }

        Value::Object(operation)
    }
}

impl Serialize for OpenApiDocument {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_value().serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_schema::SchemaNode;

    fn create_user() -> ActionDescriptor {
        ActionDescriptor::new(
            "POST",
            "/users",
            "Create a user.",
            &["auth"],
            None,
            SchemaNode::Object {
                properties: vec![(
                    "name".into(),
                    SchemaNode::String {
                        format: None,
                        pattern: None,
                    },
                )],
                required: vec!["name".into()],
            },
            SchemaNode::Bool,
        )
    }

    fn ping() -> ActionDescriptor {
        ActionDescriptor::new(
            "GET",
            "/ping",
            "",
            &[],
            None,
            SchemaNode::Any,
            SchemaNode::Any,
        )
    }

    #[test]
    fn renders_a_versioned_document_with_paths() {
        let doc = OpenApiDocument::new("Demo", "1.2.3", vec![create_user(), ping()]).to_value();
        assert_eq!(doc["openapi"], "3.1.0");
        assert_eq!(doc["info"]["title"], "Demo");
        assert_eq!(doc["info"]["version"], "1.2.3");
        assert!(doc["paths"]["/users"]["post"].is_object());
        assert!(doc["paths"]["/ping"]["get"].is_object());
    }

    #[test]
    fn a_body_action_carries_a_request_body_schema() {
        let doc = OpenApiDocument::new("Demo", "1", vec![create_user()]).to_value();
        let schema =
            &doc["paths"]["/users"]["post"]["requestBody"]["content"]["application/json"]["schema"];
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["name"]["type"], "string");
        assert_eq!(
            doc["paths"]["/users"]["post"]["responses"]["200"]["content"]["application/json"]["schema"]
                ["type"],
            "boolean"
        );
    }

    #[test]
    fn a_query_action_has_no_request_body() {
        let doc = OpenApiDocument::new("Demo", "1", vec![ping()]).to_value();
        assert!(doc["paths"]["/ping"]["get"]["requestBody"].is_null());
    }

    #[test]
    fn auth_actions_declare_security_and_a_scheme() {
        let doc = OpenApiDocument::new("Demo", "1", vec![create_user()]).to_value();
        assert_eq!(
            doc["paths"]["/users"]["post"]["security"][0]["bearerAuth"],
            json!([])
        );
        assert_eq!(
            doc["components"]["securitySchemes"]["bearerAuth"]["scheme"],
            "bearer"
        );
    }

    #[test]
    fn no_auth_means_no_components() {
        let doc = OpenApiDocument::new("Demo", "1", vec![ping()]).to_value();
        assert!(doc["components"].is_null());
    }
}
