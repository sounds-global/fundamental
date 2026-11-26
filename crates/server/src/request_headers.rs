/// The request headers captured at the boundary, matched case-insensitively.
#[derive(Debug, Clone, Default)]
pub struct RequestHeaders {
    // Names are stored lowercased, so lookups need no allocation per compare.
    headers: Vec<(String, String)>,
}

impl RequestHeaders {
    /// Creates an empty header set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the first value of the header named `name`, matched case-insensitively.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        let needle = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(key, _)| key == &needle)
            .map(|(_, value)| value.as_str())
    }

    /// Iterates every value captured for `name`, in arrival order; walkable from the back.
    pub fn get_all<'a>(&'a self, name: &str) -> impl DoubleEndedIterator<Item = &'a str> {
        let needle = name.to_ascii_lowercase();
        self.headers
            .iter()
            .filter(move |(key, _)| key == &needle)
            .map(|(_, value)| value.as_str())
    }

    /// Iterates the captured `(name, value)` pairs (names lowercased) in arrival order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.headers
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    /// Returns whether no headers were captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.headers.is_empty()
    }
}

impl fundamental_observability::trace_context::Extractor for RequestHeaders {
    /// Reads a propagation header, case-insensitively.
    fn get(&self, key: &str) -> Option<&str> {
        RequestHeaders::get(self, key)
    }

    fn keys(&self) -> Vec<&str> {
        self.headers.iter().map(|(name, _)| name.as_str()).collect()
    }
}

impl FromIterator<(String, String)> for RequestHeaders {
    /// Collects `(name, value)` pairs, lowercasing each name.
    fn from_iter<I: IntoIterator<Item = (String, String)>>(iter: I) -> Self {
        Self {
            headers: iter
                .into_iter()
                .map(|(name, value)| (name.to_ascii_lowercase(), value))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers() -> RequestHeaders {
        [
            ("DPoP".to_owned(), "proof-jws".to_owned()),
            ("Content-Type".to_owned(), "application/json".to_owned()),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn get_is_case_insensitive() {
        let h = headers();
        assert_eq!(h.get("DPoP"), Some("proof-jws"));
        assert_eq!(h.get("dpop"), Some("proof-jws"));
        assert_eq!(h.get("DPOP"), Some("proof-jws"));
        assert_eq!(h.get("content-type"), Some("application/json"));
        assert_eq!(h.get("missing"), None);
    }

    #[test]
    fn empty_headers_report_empty() {
        let h = RequestHeaders::new();
        assert!(h.is_empty());
        assert_eq!(h.get("dpop"), None);
    }

    #[test]
    fn get_all_walks_backwards_from_the_last_value() {
        let headers: RequestHeaders = [
            ("X-Forwarded-For".to_owned(), "198.51.100.1".to_owned()),
            ("Accept".to_owned(), "*/*".to_owned()),
            ("x-forwarded-for".to_owned(), "203.0.113.7".to_owned()),
        ]
        .into_iter()
        .collect();
        let backwards: Vec<&str> = headers.get_all("x-forwarded-for").rev().collect();
        assert_eq!(backwards, ["203.0.113.7", "198.51.100.1"]);
    }

    #[test]
    fn get_all_yields_every_value_in_arrival_order() {
        let h: RequestHeaders = [
            ("Cookie".to_owned(), "a=1".to_owned()),
            ("cookie".to_owned(), "b=2".to_owned()),
        ]
        .into_iter()
        .collect();
        assert_eq!(h.get_all("cookie").collect::<Vec<_>>(), ["a=1", "b=2"]);
        assert_eq!(h.get("cookie"), Some("a=1"));
        assert_eq!(h.get_all("missing").count(), 0);
    }

    #[test]
    fn first_value_wins_for_a_repeated_header() {
        let h: RequestHeaders = [
            ("x-forwarded-for".to_owned(), "1.1.1.1".to_owned()),
            ("X-Forwarded-For".to_owned(), "2.2.2.2".to_owned()),
        ]
        .into_iter()
        .collect();
        assert_eq!(h.get("x-forwarded-for"), Some("1.1.1.1"));
    }
}
