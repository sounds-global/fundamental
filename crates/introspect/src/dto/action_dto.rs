use serde::{Deserialize, Serialize};

use crate::dto::schema_dto::SchemaDto;

/// One HTTP `#[action]` as fetched surface data; mirrors [`ActionDescriptor`](fundamental_server::ActionDescriptor).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionDto {
    /// The uppercase HTTP method token (e.g. `"POST"`).
    pub method: String,
    /// The path the action is served at.
    pub path: String,
    /// The action's documentation, from its `///` comment (empty when undocumented).
    pub doc: String,
    /// The middleware in the action's chain, in order.
    pub middleware: Vec<String>,
    /// The request body's shape.
    pub input: SchemaDto,
    /// The response body's shape.
    pub output: SchemaDto,
}

impl ActionDto {
    /// Whether the action's middleware chain includes `auth`.
    #[must_use]
    pub fn requires_auth(&self) -> bool {
        self.middleware.iter().any(|m| m == "auth")
    }

    /// Whether the method carries a request body (`POST`/`PUT`/`PATCH`).
    #[must_use]
    pub fn has_request_body(&self) -> bool {
        matches!(self.method.as_str(), "POST" | "PUT" | "PATCH")
    }
}

#[cfg(feature = "ssr")]
impl From<&fundamental_server::ActionDescriptor> for ActionDto {
    fn from(action: &fundamental_server::ActionDescriptor) -> Self {
        Self {
            method: action.method.to_owned(),
            path: action.path.to_owned(),
            doc: action.doc.to_owned(),
            middleware: action.middleware.iter().map(|&m| m.to_owned()).collect(),
            input: SchemaDto::from(&action.input),
            output: SchemaDto::from(&action.output),
        }
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;
    use fundamental_schema::SchemaNode;
    use fundamental_server::ActionDescriptor;

    #[test]
    fn converts_every_field_and_round_trips() {
        let dto = ActionDto::from(&ActionDescriptor::new(
            "POST",
            "/users",
            "Create a user.",
            &["auth"],
            None,
            SchemaNode::Bool,
            SchemaNode::Any,
        ));
        assert_eq!(dto.method, "POST");
        assert_eq!(dto.path, "/users");
        assert_eq!(dto.doc, "Create a user.");
        assert_eq!(dto.middleware, vec!["auth".to_string()]);
        assert_eq!(dto.input, SchemaDto::Bool);

        let json = serde_json::to_string(&dto).expect("serialize");
        assert_eq!(serde_json::from_str::<ActionDto>(&json).unwrap(), dto);
    }
}
