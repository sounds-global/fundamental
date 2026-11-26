use std::collections::HashMap;

/// Headers attached to a NATS message. Each key may hold multiple values.
#[derive(Debug, Clone, Default)]
pub struct MessageHeaders {
    inner: HashMap<String, Vec<String>>,
}

impl MessageHeaders {
    /// Creates empty headers.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a value under `key`.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.inner.entry(key.into()).or_default().push(value.into());
    }

    /// Returns the first value for `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.inner
            .get(key)
            .and_then(|v| v.first())
            .map(String::as_str)
    }

    /// Returns all values for `key`.
    #[must_use]
    pub fn get_all(&self, key: &str) -> Option<&[String]> {
        self.inner.get(key).map(Vec::as_slice)
    }

    /// Returns whether there are no headers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Iterates over each key and its values.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.inner.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }
}

impl fundamental_observability::trace_context::Injector for MessageHeaders {
    /// Writes `value` under `key`, replacing any values already there, so `traceparent` is never
    /// doubled.
    fn set(&mut self, key: &str, value: String) {
        self.inner.insert(key.to_string(), vec![value]);
    }
}

impl fundamental_observability::trace_context::Extractor for MessageHeaders {
    fn get(&self, key: &str) -> Option<&str> {
        MessageHeaders::get(self, key)
    }

    fn keys(&self) -> Vec<&str> {
        self.inner.keys().map(String::as_str).collect()
    }
}

impl From<MessageHeaders> for async_nats::HeaderMap {
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn from(headers: MessageHeaders) -> Self {
        let mut map = async_nats::HeaderMap::new();
        for (key, values) in headers.inner {
            let Ok(name) = key.parse::<async_nats::HeaderName>() else {
                continue;
            };
            for value in values {
                if let Ok(header_value) = value.parse::<async_nats::HeaderValue>() {
                    map.append(name.clone(), header_value);
                }
            }
        }
        map
    }
}

impl From<&async_nats::HeaderMap> for MessageHeaders {
    fn from(map: &async_nats::HeaderMap) -> Self {
        let mut headers = MessageHeaders::new();
        for (key, values) in map.iter() {
            let key = key.to_string();
            for value in values {
                headers.insert(&key, value.to_string());
            }
        }
        headers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_valued_insert_and_get() {
        let mut h = MessageHeaders::new();
        h.insert("X-Custom", "v1");
        h.insert("X-Custom", "v2");
        assert_eq!(h.get("X-Custom"), Some("v1"));
        assert_eq!(
            h.get_all("X-Custom"),
            Some(&["v1".to_string(), "v2".to_string()][..])
        );
        assert!(h.get("absent").is_none());
        assert!(!h.is_empty());
    }

    #[test]
    fn roundtrips_through_nats_header_map() {
        let mut h = MessageHeaders::new();
        h.insert("X-A", "1");
        h.insert("X-A", "2");
        h.insert("X-B", "3");
        let map: async_nats::HeaderMap = h.into();
        let back = MessageHeaders::from(&map);
        assert_eq!(back.get_all("X-A").map(<[_]>::len), Some(2));
        assert_eq!(back.get("X-B"), Some("3"));
    }
}
