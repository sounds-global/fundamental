use serde::Serialize;

use crate::web_client_error::WebClientError;

/// What a call to an action sends: the path, the query string and the body.
///
/// A `GET` or `DELETE` carries its input in the query string, any other method as JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRequest {
    path: String,
    query: String,
    body: Option<String>,
}

impl ActionRequest {
    /// Shapes a call of `method` (an uppercase HTTP token such as `"GET"`) to `path` with `input`.
    ///
    /// # Errors
    ///
    /// [`WebClientError::Transport`] when `path` still holds a `{capture}`, naming each one,
    /// or when the input does not serialize: a `GET` or `DELETE` input must be a flat struct
    /// of scalar fields (or `()`), as a query string holds.
    pub fn new<I: Serialize>(method: &str, path: &str, input: &I) -> Result<Self, WebClientError> {
        let captures = unfilled_captures(path);
        if !captures.is_empty() {
            return Err(WebClientError::Transport(format!(
                "`{path}` still holds {}: call the action at the path its marker fills, \
                 with `.at(..)` on its hook",
                captures.join(", ")
            )));
        }
        let in_query = method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("DELETE");
        let (query, body) = match in_query {
            true => (
                serde_urlencoded::to_string(input).map_err(|error| {
                    WebClientError::Transport(format!("serialize input as a query: {error}"))
                })?,
                None,
            ),
            false => (
                String::new(),
                Some(serde_json::to_string(input).map_err(|error| {
                    WebClientError::Transport(format!("serialize input: {error}"))
                })?),
            ),
        };
        Ok(Self {
            path: path.to_owned(),
            query,
            body,
        })
    }

    /// The action's path, as in `/teams/acme/sessions`.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The query string without its `?`; empty when the call carries none.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The JSON body; `None` for a `GET` or `DELETE`, whose input is in the query.
    #[must_use]
    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }

    /// The address to request: the path, then `?` and the query when there is one.
    #[must_use]
    pub fn target(&self) -> String {
        match self.query.is_empty() {
            true => self.path.clone(),
            false => format!("{}?{}", self.path, self.query),
        }
    }
}

fn unfilled_captures(path: &str) -> Vec<&str> {
    path.split('/')
        .filter(|segment| segment.starts_with('{') && segment.ends_with('}'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    #[derive(Serialize)]
    struct Filter {
        active: bool,
        name: &'static str,
    }

    const FILTER: Filter = Filter {
        active: true,
        name: "a b&c",
    };

    #[test]
    fn a_get_sends_its_input_as_the_query_string() {
        let request = ActionRequest::new("GET", "/teams/acme/sessions", &FILTER).expect("shaped");
        assert_eq!(request.query(), "active=true&name=a+b%26c");
        assert_eq!(request.body(), None);
        assert_eq!(
            request.target(),
            "/teams/acme/sessions?active=true&name=a+b%26c"
        );
    }

    #[test]
    fn a_delete_sends_its_input_as_the_query_string() {
        let request = ActionRequest::new("DELETE", "/passkeys/k1", &FILTER).expect("shaped");
        assert_eq!(request.target(), "/passkeys/k1?active=true&name=a+b%26c");
        assert_eq!(request.body(), None);
    }

    #[test]
    fn a_post_sends_its_input_as_a_json_body() {
        let request = ActionRequest::new("POST", "/teams", &FILTER).expect("shaped");
        assert_eq!(request.target(), "/teams");
        assert_eq!(request.body(), Some(r#"{"active":true,"name":"a b&c"}"#));
    }

    #[test]
    fn a_get_without_input_sends_no_query() {
        let request = ActionRequest::new("GET", "/teams", &()).expect("shaped");
        assert_eq!(request.query(), "");
        assert_eq!(request.target(), "/teams");
    }

    #[test]
    fn a_filled_capture_is_sent_as_its_escaped_form() {
        let request = ActionRequest::new("GET", "/teams/a%2Fb", &()).expect("shaped");
        assert_eq!(request.path(), "/teams/a%2Fb");
    }

    #[test]
    fn a_template_with_captures_is_refused() {
        let error = ActionRequest::new("GET", "/teams/{team}/passkeys/{key}", &())
            .expect_err("a template is no address");
        assert_eq!(error.error_code(), "WEB_001");
        let WebClientError::Transport(detail) = error else {
            panic!("a transport refusal, got {error:?}");
        };
        assert!(detail.contains("{team}, {key}"), "{detail}");
        assert!(detail.contains(".at(..)"), "{detail}");
    }

    #[test]
    fn a_query_input_that_is_not_flat_is_refused() {
        #[derive(Serialize)]
        struct Nested {
            inner: Filter,
        }
        let error = ActionRequest::new("GET", "/teams", &Nested { inner: FILTER })
            .expect_err("a query holds scalars");
        assert!(matches!(error, WebClientError::Transport(_)));
    }
}
