use super::MessageHeaders;
use crate::subject::Subject;

/// The delivery facts of a request-reply message: its headers and the subject it
/// arrived on.
///
/// A `#[respond]` handler takes this as an optional third parameter:
///
/// ```ignore
/// #[respond(queue = Erasers)]
/// async fn erase_rpc(state: &State, request: Request, meta: &RespondMeta) -> Reply {
///     let bearer = meta.header("Authorization");
///     // …
/// }
/// ```
///
///
/// For facts that must not be in the payload, such as a credential, since a payload is kept,
/// published as a schema and logged.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RespondMeta {
    /// The headers the request carried.
    pub headers: MessageHeaders,
    /// The concrete subject the request arrived on.
    pub subject: Subject,
}

impl RespondMeta {
    /// Builds the metadata for one delivery. Called by `#[respond]`-generated code.
    #[must_use]
    pub fn new(headers: MessageHeaders, subject: Subject) -> Self {
        Self { headers, subject }
    }

    /// Returns the first value of `key`, if the request carried it.
    #[must_use]
    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers.get(key)
    }

    /// Returns the bearer token from an `Authorization` header, without the scheme.
    ///
    /// Matches `Bearer ` and `bearer ` only; `None` for a missing header, another scheme, or an
    /// empty token.
    #[must_use]
    pub fn bearer_token(&self) -> Option<&str> {
        let value = self.header("Authorization")?;
        let token = value
            .strip_prefix("Bearer ")
            .or_else(|| value.strip_prefix("bearer "))?
            .trim();
        (!token.is_empty()).then_some(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(header: Option<(&str, &str)>) -> RespondMeta {
        let mut headers = MessageHeaders::new();
        if let Some((key, value)) = header {
            headers.insert(key, value);
        }
        RespondMeta::new(headers, Subject::new_unchecked("rpc.test"))
    }

    #[test]
    fn a_header_is_readable_by_name() {
        assert_eq!(
            meta(Some(("Idempotency-Key", "abc"))).header("Idempotency-Key"),
            Some("abc")
        );
    }

    #[test]
    fn a_missing_header_is_none() {
        assert_eq!(meta(None).header("Idempotency-Key"), None);
    }

    #[test]
    fn a_bearer_token_is_extracted_without_its_scheme() {
        assert_eq!(
            meta(Some(("Authorization", "Bearer tok-1"))).bearer_token(),
            Some("tok-1")
        );
    }

    #[test]
    fn a_lowercase_bearer_scheme_is_accepted() {
        assert_eq!(
            meta(Some(("Authorization", "bearer tok-1"))).bearer_token(),
            Some("tok-1")
        );
    }

    #[test]
    fn another_scheme_is_not_a_bearer_token() {
        assert_eq!(
            meta(Some(("Authorization", "Basic dXNlcjpwYXNz"))).bearer_token(),
            None
        );
    }

    /// An empty token never reaches a verifier.
    #[test]
    fn an_empty_bearer_token_is_none() {
        assert_eq!(
            meta(Some(("Authorization", "Bearer   "))).bearer_token(),
            None
        );
    }

    #[test]
    fn no_authorization_header_is_none() {
        assert_eq!(meta(None).bearer_token(), None);
    }

    #[test]
    fn the_subject_the_request_arrived_on_is_carried() {
        assert_eq!(meta(None).subject.as_str(), "rpc.test");
    }
}
