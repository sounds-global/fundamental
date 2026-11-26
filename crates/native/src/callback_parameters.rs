use fundamental_core::secrets::{InMemorySecret, InMemorySecretString};

/// The query parameters an RFC 8252 loopback redirect arrived with.
///
/// Every value is an [`InMemorySecretString`], redacted in `Debug` and never stored. Whether the
/// success or the `error` parameter arrived is the caller's question.
#[derive(Debug, Default)]
pub struct CallbackParameters {
    entries: Vec<(String, InMemorySecretString)>,
}

impl CallbackParameters {
    /// The value delivered under `name`, if one was.
    ///
    /// The first occurrence wins.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&InMemorySecretString> {
        self.entries
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    /// Whether a parameter of this name arrived at all.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// How many parameters arrived.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the redirect carried no parameters at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Parse a raw query string, keeping at most `limit` parameters.
    ///
    /// Decoded by `form_urlencoded`, the WHATWG algorithm browsers write with; `limit` bounds
    /// a query an attacker writes.
    pub(crate) fn parse(query: &str, limit: usize) -> Self {
        let entries = form_urlencoded::parse(query.as_bytes())
            .take(limit)
            .map(|(name, value)| (name.into_owned(), InMemorySecret::new(value.into_owned())))
            .collect();
        Self { entries }
    }
}

#[cfg(test)]
mod tests {
    use super::CallbackParameters;

    #[test]
    fn a_parameter_is_read_back_decoded() {
        let parameters = CallbackParameters::parse("code=ab%2Fcd&state=one+two", 16);
        assert_eq!(
            parameters
                .get("code")
                .and_then(|code| code.expose())
                .map(String::as_str),
            Some("ab/cd")
        );
        assert_eq!(
            parameters
                .get("state")
                .and_then(|state| state.expose())
                .map(String::as_str),
            Some("one two")
        );
    }

    #[test]
    fn a_parameter_that_did_not_arrive_is_absent_rather_than_empty() {
        let parameters = CallbackParameters::parse("error=access_denied", 16);
        assert!(parameters.contains("error"));
        assert!(!parameters.contains("code"));
        assert!(parameters.get("code").is_none());
    }

    #[test]
    fn the_parse_stops_at_the_limit() {
        let query: String = (0..64).map(|index| format!("p{index}=v{index}&")).collect();
        let parameters = CallbackParameters::parse(&query, 16);
        assert_eq!(parameters.len(), 16);
    }

    /// A repeated parameter resolves to its first occurrence.
    #[test]
    fn a_repeated_parameter_resolves_to_the_first() {
        let parameters = CallbackParameters::parse("code=first&code=second", 16);
        assert_eq!(
            parameters
                .get("code")
                .and_then(|code| code.expose())
                .map(String::as_str),
            Some("first")
        );
    }

    /// The container redacts in `Debug`.
    #[test]
    fn debug_carries_no_value() {
        let parameters = CallbackParameters::parse("code=super-secret-grant", 16);
        let rendered = format!("{parameters:?}");
        assert!(
            !rendered.contains("super-secret-grant"),
            "a callback parameter reached a Debug rendering: {rendered}"
        );
    }

    #[test]
    fn an_empty_query_is_empty() {
        let parameters = CallbackParameters::parse("", 16);
        assert!(parameters.is_empty());
        assert_eq!(parameters.len(), 0);
    }
}
