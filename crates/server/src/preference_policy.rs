use crate::cookie_lookup::CookieLookup;

/// One display preference: a named, closed set of values a reader may pin, read from a cookie.
///
/// Absent, unoffered and ambiguous are all no preference. Never put anything that carries
/// authority in one.
///
/// ```
/// # extern crate fundamental_server;
/// use fundamental_server::PreferencePolicy;
///
/// let appearance = PreferencePolicy::new("appearance", "site-appearance", &["light", "dark"]);
///
/// assert_eq!(appearance.read(["site-appearance=dark"].into_iter()), Some("dark"));
/// // A value this build does not offer is the same as none.
/// assert_eq!(appearance.read(["site-appearance=sepia"].into_iter()), None);
/// // And so is the same name arriving twice with different values.
/// assert_eq!(
///     appearance.read(["site-appearance=dark; site-appearance=light"].into_iter()),
///     None
/// );
/// ```
#[derive(Debug, Clone)]
pub struct PreferencePolicy {
    name: &'static str,
    cookie: String,
    allowed: &'static [&'static str],
}

impl PreferencePolicy {
    /// A preference called `name`, read from `cookie`, offering exactly `allowed`.
    #[must_use]
    pub fn new(
        name: &'static str,
        cookie: impl Into<String>,
        allowed: &'static [&'static str],
    ) -> Self {
        Self {
            name,
            cookie: cookie.into(),
            allowed,
        }
    }

    /// What a page asks for.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The cookie this preference is read from.
    #[must_use]
    pub fn cookie(&self) -> &str {
        &self.cookie
    }

    /// The values this build offers, in the order they were declared.
    #[must_use]
    pub const fn allowed(&self) -> &'static [&'static str] {
        self.allowed
    }

    /// The value this request pinned, as one of this build's own strings, if it pinned one it offers.
    #[must_use]
    pub fn read<'a>(&self, cookie_fields: impl Iterator<Item = &'a str>) -> Option<&'static str> {
        match CookieLookup::find(cookie_fields, &self.cookie) {
            CookieLookup::Found(value) => self
                .allowed
                .iter()
                .copied()
                .find(|offered| *offered == value),
            CookieLookup::Absent | CookieLookup::Ambiguous(_) => None,
        }
    }

    /// The request headers a response produced under this policy varies on: `Cookie`, on every representation.
    #[must_use]
    pub const fn vary(&self) -> &'static [&'static str] {
        &["Cookie"]
    }
}

#[cfg(test)]
mod tests {
    use super::PreferencePolicy;

    fn appearance() -> PreferencePolicy {
        PreferencePolicy::new("appearance", "site-appearance", &["light", "dark"])
    }

    #[test]
    fn an_offered_value_resolves_and_an_unoffered_one_does_not() {
        assert_eq!(
            appearance().read(["site-appearance=light"].into_iter()),
            Some("light")
        );
        assert_eq!(
            appearance().read(["site-appearance=sepia"].into_iter()),
            None
        );
        assert_eq!(appearance().read(["other=dark"].into_iter()), None);
        assert_eq!(appearance().read([].into_iter()), None);
    }

    #[test]
    fn the_same_name_twice_resolves_to_nothing() {
        assert_eq!(
            appearance().read(["site-appearance=dark", "site-appearance=light"].into_iter()),
            None
        );
    }

    /// The same value twice is not ambiguous — it is one preference, stated twice.
    #[test]
    fn the_same_value_twice_is_not_an_ambiguity() {
        assert_eq!(
            appearance().read(["site-appearance=dark", "site-appearance=dark"].into_iter()),
            Some("dark")
        );
    }

    #[test]
    fn the_value_handed_out_is_the_builds_own() {
        let policy = appearance();
        let resolved = policy
            .read(["site-appearance=dark"].into_iter())
            .expect("a value");
        assert!(
            policy
                .allowed()
                .iter()
                .any(|offered| std::ptr::eq(*offered, resolved))
        );
    }

    #[test]
    fn every_representation_varies_on_the_cookie() {
        assert_eq!(appearance().vary(), ["Cookie"]);
    }
}
