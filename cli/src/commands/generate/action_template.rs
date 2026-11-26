//! Source generation for `fun generate action`.

/// An action source file: the request/response types and the server-only `#[action]` handler.
pub(super) fn action_source(snake: &str, pascal: &str) -> String {
    format!(
        "//! The `{snake}` action.\n\n\
         use serde::{{Deserialize, Serialize}};\n\n\
         /// Request for the `{snake}` action. Edit the fields to suit.\n\
         #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]\n\
         pub struct {pascal}Request {{\n\
         \x20   /// Replace with the action's input.\n\
         \x20   pub name: String,\n\
         }}\n\n\
         /// Response from the `{snake}` action. Edit the fields to suit.\n\
         #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]\n\
         pub struct {pascal}Response {{\n\
         \x20   /// Replace with the action's output.\n\
         \x20   pub message: String,\n\
         }}\n\n\
         // Serve it by listing `{pascal}` in a surface's `mount_actions!`; paths sit under `/api/`.\n\
         use fundamental::{{action, AnyError}};\n\n\
         #[action(POST, \"/api/{snake}\", reactive)]\n\
         pub async fn {snake}(\n\
         \x20   _context: HttpContext<()>,\n\
         \x20   request: {pascal}Request,\n\
         ) -> Result<{pascal}Response, AnyError> {{\n\
         \x20   Ok({pascal}Response {{ message: format!(\"hello, {{}}\", request.name) }})\n\
         }}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_dtos_and_a_plain_action_handler() {
        let src = action_source("greet", "Greet");
        assert!(src.contains("pub struct GreetRequest {"), "{src}");
        assert!(src.contains("pub struct GreetResponse {"), "{src}");
        assert!(
            src.contains("#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]"),
            "{src}"
        );
        assert!(!src.contains("#[cfg"), "{src}");
        assert!(!src.contains("feature = \"server\""), "{src}");
        assert!(
            src.contains("use fundamental::{action, AnyError};"),
            "{src}"
        );
        assert!(
            src.contains("#[action(POST, \"/api/greet\", reactive)]"),
            "{src}"
        );
        assert!(
            src.contains("pub async fn greet(") && src.contains("HttpContext<()>"),
            "{src}"
        );
        assert!(src.contains("Result<GreetResponse, AnyError>"), "{src}");
        syn::parse_file(&src).expect("generated action source must parse");
    }

    #[test]
    fn a_multiword_name_pascalizes_the_dtos() {
        let src = action_source("create_user", "CreateUser");
        assert!(src.contains("pub struct CreateUserRequest {"), "{src}");
        assert!(src.contains("pub async fn create_user("), "{src}");
        assert!(src.contains("\"/api/create_user\""), "{src}");
    }
}
