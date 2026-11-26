use std::time::Duration;

/// A page's cache rule; a request with a principal or a guarded route is always [`NoStore`](Self::NoStore).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cache {
    /// Any cache may keep it for `max_age`.
    Public {
        /// How long a cache may serve it without asking again.
        max_age: Duration,
    },
    /// Only the reader's own browser may keep it, and it asks again before reusing it.
    Private,
    /// Nobody keeps it. The default, and the floor of every personal page.
    NoStore,
}

impl Cache {
    /// The stricter of two rules: `NoStore` over `Private` over `Public`, then the shorter.
    #[must_use]
    pub(crate) fn stricter(self, other: Self) -> Self {
        match (self, other) {
            (Self::NoStore, _) | (_, Self::NoStore) => Self::NoStore,
            (Self::Private, _) | (_, Self::Private) => Self::Private,
            (Self::Public { max_age: a }, Self::Public { max_age: b }) => {
                Self::Public { max_age: a.min(b) }
            }
        }
    }

    #[must_use]
    pub(crate) fn header_value(self) -> String {
        match self {
            Self::Public { max_age } => format!("public, max-age={}", max_age.as_secs()),
            Self::Private => "private, no-cache".to_owned(),
            Self::NoStore => "no-store".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Cache;
    use std::time::Duration;

    fn public(secs: u64) -> Cache {
        Cache::Public {
            max_age: Duration::from_secs(secs),
        }
    }

    #[test]
    fn the_stricter_rule_wins_whichever_side_it_is_on() {
        assert_eq!(public(60).stricter(Cache::NoStore), Cache::NoStore);
        assert_eq!(Cache::NoStore.stricter(Cache::Private), Cache::NoStore);
        assert_eq!(public(60).stricter(Cache::Private), Cache::Private);
        assert_eq!(Cache::Private.stricter(public(60)), Cache::Private);
        assert_eq!(public(600).stricter(public(60)), public(60));
    }

    #[test]
    fn each_rule_has_its_header() {
        assert_eq!(public(300).header_value(), "public, max-age=300");
        assert_eq!(Cache::Private.header_value(), "private, no-cache");
        assert_eq!(Cache::NoStore.header_value(), "no-store");
    }
}
