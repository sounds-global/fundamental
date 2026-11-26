//! [`ScopeSet`] — the scopes a typed value denotes.

/// A set of scopes, named by a typed value rather than a string literal.
///
/// A route requires and a token grants the same checked value; several scopes are a conjunction.
///
/// ```
/// # extern crate fundamental_core;
/// use fundamental_core::ScopeSet;
///
/// enum Subject {
///     Erase,
/// }
///
/// impl ScopeSet for Subject {
///     fn required_scopes(&self) -> &'static [&'static str] {
///         match self {
///             Self::Erase => &["subject:erase"],
///         }
///     }
/// }
///
/// assert_eq!(Subject::Erase.required_scopes(), &["subject:erase"]);
/// ```
pub trait ScopeSet {
    /// The scopes this value denotes, as the strings that travel on the wire.
    fn required_scopes(&self) -> &'static [&'static str];
}

/// Collects [`ScopeSet`] values into the `Vec<String>` a token is minted with.
///
/// ```
/// # extern crate fundamental_core;
/// use fundamental_core::{ScopeSet, scopes};
///
/// # enum Subject { Erase }
/// # impl ScopeSet for Subject {
/// #     fn required_scopes(&self) -> &'static [&'static str] { &["subject:erase"] }
/// # }
/// # enum Credential { Revoke }
/// # impl ScopeSet for Credential {
/// #     fn required_scopes(&self) -> &'static [&'static str] { &["credential:revoke"] }
/// # }
/// // Heterogeneous on purpose: an operator bearer routinely spans several
/// // vocabularies, which no single `&[S]` could hold.
/// let granted = scopes![Subject::Erase, Credential::Revoke];
/// assert_eq!(granted, vec!["subject:erase", "credential:revoke"]);
/// ```
#[macro_export]
macro_rules! scopes {
    () => { ::std::vec::Vec::<::std::string::String>::new() };
    ($($set:expr),+ $(,)?) => {{
        let mut granted = ::std::vec::Vec::<::std::string::String>::new();
        $(
            granted.extend(
                $crate::ScopeSet::required_scopes(&$set)
                    .iter()
                    .map(|scope| ::std::string::ToString::to_string(scope)),
            );
        )+
        granted
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    enum Subject {
        Erase,
    }

    impl ScopeSet for Subject {
        fn required_scopes(&self) -> &'static [&'static str] {
            match self {
                Self::Erase => &["subject:erase"],
            }
        }
    }

    enum Credential {
        Revoke,
        Lifecycle,
    }

    impl ScopeSet for Credential {
        fn required_scopes(&self) -> &'static [&'static str] {
            match self {
                Self::Revoke => &["credential:revoke"],
                Self::Lifecycle => &["credential:revoke", "credential:suspend"],
            }
        }
    }

    #[test]
    fn a_single_value_denotes_one_scope() {
        assert_eq!(scopes![Subject::Erase], vec!["subject:erase"]);
    }

    #[test]
    fn values_from_different_vocabularies_collect_together() {
        assert_eq!(
            scopes![Subject::Erase, Credential::Revoke],
            vec!["subject:erase", "credential:revoke"]
        );
    }

    #[test]
    fn a_conjunction_contributes_every_scope_it_names() {
        assert_eq!(
            scopes![Credential::Lifecycle],
            vec!["credential:revoke", "credential:suspend"]
        );
    }

    #[test]
    fn granting_nothing_is_an_empty_grant() {
        let granted: Vec<String> = scopes![];
        assert!(granted.is_empty());
    }

    #[test]
    fn a_trailing_comma_is_accepted() {
        assert_eq!(scopes![Subject::Erase,], vec!["subject:erase"]);
    }
}
