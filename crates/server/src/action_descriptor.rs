use fundamental_schema::SchemaNode;

/// A machine-readable description of one HTTP action: how it is addressed, what it carries, what guards it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ActionDescriptor {
    /// The uppercase HTTP method token (e.g. `"POST"`).
    pub method: &'static str,
    /// The path the action is served at (e.g. `"/api/users"`).
    pub path: &'static str,
    /// The action's documentation, from its `///` comment (empty when undocumented).
    pub doc: &'static str,
    /// The names of the middleware in the action's chain, in order (e.g. `["auth"]`).
    pub middleware: &'static [&'static str],
    /// Why this action's act names no single object, or `None` when it names one.
    pub no_single_object: Option<&'static str>,
    /// The request body's shape ([`SchemaNode::Any`] when the type has no derived schema).
    pub input: SchemaNode,
    /// The response body's shape.
    pub output: SchemaNode,
}

impl ActionDescriptor {
    /// Builds a descriptor. Called by `mount_actions!`-generated code.
    #[must_use]
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    pub fn new(
        method: &'static str,
        path: &'static str,
        doc: &'static str,
        middleware: &'static [&'static str],
        no_single_object: Option<&'static str>,
        input: SchemaNode,
        output: SchemaNode,
    ) -> Self {
        Self {
            method,
            path,
            doc,
            middleware,
            no_single_object,
            input,
            output,
        }
    }

    /// Whether the action's middleware chain includes `auth`.
    #[must_use]
    pub fn requires_auth(&self) -> bool {
        self.middleware.contains(&"auth")
    }

    /// Whether the action's method carries a request body (`POST`/`PUT`/`PATCH`).
    #[must_use]
    pub fn has_request_body(&self) -> bool {
        matches!(self.method, "POST" | "PUT" | "PATCH")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ActionDescriptor {
        ActionDescriptor::new(
            "POST",
            "/users",
            "Create a user.",
            &["auth"],
            None,
            SchemaNode::Any,
            SchemaNode::Any,
        )
    }

    #[test]
    fn auth_is_detected_from_the_middleware_chain() {
        assert!(sample().requires_auth());
        let open = ActionDescriptor::new(
            "GET",
            "/ping",
            "",
            &[],
            None,
            SchemaNode::Any,
            SchemaNode::Any,
        );
        assert!(!open.requires_auth());
    }

    #[test]
    fn body_methods_are_distinguished_from_query_methods() {
        assert!(sample().has_request_body());
        let get = ActionDescriptor::new(
            "GET",
            "/ping",
            "",
            &[],
            None,
            SchemaNode::Any,
            SchemaNode::Any,
        );
        assert!(!get.has_request_body());
    }
}
