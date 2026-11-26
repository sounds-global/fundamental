#[cfg(not(feature = "std"))]
use alloc::collections::BTreeMap;
#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::collections::BTreeMap;

/// The process environment, or a stand-in for it.
///
/// Configuration reads through it so a test needs no `unsafe` `set_var`.
///
/// # Example
///
/// ```
/// use fundamental_core::env::EnvVars;
///
/// let vars = EnvVars::fixed([("FUNDAMENTAL_CUSTOM__ORIGINS__WWW", "https://example.test")]);
/// assert_eq!(
///     vars.get("FUNDAMENTAL_CUSTOM__ORIGINS__WWW").as_deref(),
///     Some("https://example.test"),
/// );
/// assert!(vars.get("PATH").is_none(), "a fixed set is the whole environment, not an overlay");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnvVars {
    /// `None` is the real process environment; `Some` is a fixed stand-in.
    /// `None` is the real process environment; `Some` is the whole stand-in, never an overlay.
    fixed: Option<BTreeMap<String, String>>,
}

impl EnvVars {
    /// The real process environment, the default.
    #[must_use]
    pub const fn process() -> Self {
        Self { fixed: None }
    }

    /// A fixed environment: exactly these variables and nothing else.
    pub fn fixed<K, V>(pairs: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            fixed: Some(
                pairs
                    .into_iter()
                    .map(|(key, value)| (key.into(), value.into()))
                    .collect(),
            ),
        }
    }

    /// An empty fixed environment — nothing is set.
    #[must_use]
    pub fn none() -> Self {
        Self::fixed(core::iter::empty::<(String, String)>())
    }

    /// The value of one variable, if it is set.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<String> {
        match &self.fixed {
            Some(fixed) => fixed.get(key).cloned(),
            #[cfg(feature = "std")]
            None => std::env::var(key).ok(),
            #[cfg(not(feature = "std"))]
            None => None,
        }
    }

    /// Every variable, as `(name, value)`.
    #[must_use]
    pub fn entries(&self) -> Vec<(String, String)> {
        match &self.fixed {
            Some(fixed) => fixed
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            #[cfg(feature = "std")]
            None => std::env::vars().collect(),
            #[cfg(not(feature = "std"))]
            None => Vec::new(),
        }
    }

    /// Whether this reads the real process environment.
    #[must_use]
    pub const fn is_process(&self) -> bool {
        self.fixed.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::EnvVars;
    #[cfg(not(feature = "std"))]
    use alloc::string::{String, ToString as _};
    #[cfg(not(feature = "std"))]
    use alloc::vec::Vec;

    #[test]
    fn the_default_is_the_process_environment() {
        assert!(EnvVars::default().is_process());
        assert!(EnvVars::process().is_process());
    }

    #[test]
    fn a_fixed_set_answers_only_what_it_was_given() {
        let vars = EnvVars::fixed([("A", "1"), ("B", "2")]);
        assert_eq!(vars.get("A").as_deref(), Some("1"));
        assert_eq!(vars.get("C"), None);
        assert!(!vars.is_process());
    }

    #[test]
    fn a_fixed_set_is_not_an_overlay_on_the_real_environment() {
        let vars = EnvVars::fixed([("A", "1")]);
        assert_eq!(
            vars.entries(),
            Vec::from([("A".to_string(), "1".to_string())])
        );
        assert!(vars.get("PATH").is_none());
    }

    #[test]
    fn an_empty_environment_is_expressible() {
        let vars = EnvVars::none();
        assert!(vars.entries().is_empty());
        assert_eq!(vars.get("PATH"), None);
        assert!(!vars.is_process());
    }

    #[test]
    fn entries_round_trip_through_fixed() {
        let vars = EnvVars::fixed([("B", "2"), ("A", "1")]);
        let entries: Vec<(String, String)> = vars.entries();
        assert_eq!(entries[0].0, "A");
        assert_eq!(entries[1].0, "B");
    }
}
