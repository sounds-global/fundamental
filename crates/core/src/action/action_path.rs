//! [`ActionPath`] — one action's path with each capture filled in its public form.

#[cfg(not(feature = "std"))]
use alloc::string::String;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;
use core::ops::Deref;

/// The path of action `M` with every capture filled, as `M::path(..)` builds it.
///
/// The marker ties the path to its action, so a path built for one action is never sent to another.
pub struct ActionPath<M> {
    path: String,
    marker: PhantomData<fn() -> M>,
}

impl<M> ActionPath<M> {
    /// Wraps a path `#[action]` filled for marker `M`.
    #[doc(hidden)]
    #[must_use]
    pub fn filled(path: String) -> Self {
        Self {
            path,
            marker: PhantomData,
        }
    }

    /// The filled path, as in `/teams/acme/sessions`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.path
    }

    /// The filled path as an owned string.
    #[must_use]
    pub fn into_string(self) -> String {
        self.path
    }
}

impl<M> Deref for ActionPath<M> {
    type Target = str;

    fn deref(&self) -> &str {
        &self.path
    }
}

impl<M> AsRef<str> for ActionPath<M> {
    fn as_ref(&self) -> &str {
        &self.path
    }
}

impl<M> fmt::Display for ActionPath<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.path)
    }
}

impl<M> fmt::Debug for ActionPath<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ActionPath").field(&self.path).finish()
    }
}

impl<M> Clone for ActionPath<M> {
    fn clone(&self) -> Self {
        Self::filled(self.path.clone())
    }
}

impl<M> PartialEq for ActionPath<M> {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl<M> Eq for ActionPath<M> {}

impl<M> Hash for ActionPath<M> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
    }
}

impl<M> PartialEq<str> for ActionPath<M> {
    fn eq(&self, other: &str) -> bool {
        self.path == other
    }
}

impl<M> PartialEq<&str> for ActionPath<M> {
    fn eq(&self, other: &&str) -> bool {
        self.path == *other
    }
}

impl<M> From<ActionPath<M>> for String {
    fn from(path: ActionPath<M>) -> Self {
        path.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Marker;

    #[test]
    fn a_path_reads_as_the_string_it_holds() {
        let path = ActionPath::<Marker>::filled("/teams/acme".to_owned());
        assert_eq!(&*path, "/teams/acme");
        assert_eq!(path.as_ref(), "/teams/acme");
        assert_eq!(path, "/teams/acme");
        assert_eq!(format!("{path:?}"), "ActionPath(\"/teams/acme\")");
        assert_eq!(path.clone().into_string(), "/teams/acme");
    }

    #[test]
    fn two_paths_are_equal_only_when_their_text_is() {
        let acme = ActionPath::<Marker>::filled("/teams/acme".to_owned());
        assert_eq!(acme, acme.clone());
        assert_ne!(acme, ActionPath::filled("/teams/other".to_owned()));
    }
}
