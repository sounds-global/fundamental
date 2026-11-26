/// Produces `Self` from a reference to a larger state, so one surface can serve several bounded contexts.
///
/// `#[action]` implements [`ServerAction<Outer>`](crate::ServerAction) for every `Outer` its
/// state is read out of; the blanket `impl<T: Clone> FromRef<T> for T` keeps a single-context
/// surface unchanged.
///
/// ```
/// # use fundamental_server::FromRef;
/// #[derive(Clone)]
/// struct Identity;
/// #[derive(Clone)]
/// struct Organisations;
///
/// #[derive(Clone)]
/// struct Portal {
///     identity: Identity,
///     organisations: Organisations,
/// }
///
/// impl FromRef<Portal> for Identity {
///     fn from_ref(portal: &Portal) -> Self {
///         portal.identity.clone()
///     }
/// }
///
/// impl FromRef<Portal> for Organisations {
///     fn from_ref(portal: &Portal) -> Self {
///         portal.organisations.clone()
///     }
/// }
/// ```
pub trait FromRef<Outer> {
    /// Reads this state out of `outer`.
    fn from_ref(outer: &Outer) -> Self;
}

impl<T: Clone> FromRef<T> for T {
    fn from_ref(outer: &T) -> Self {
        outer.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::FromRef;

    #[derive(Clone, Debug, PartialEq)]
    struct Identity(&'static str);

    #[derive(Clone, Debug, PartialEq)]
    struct Organisations(&'static str);

    #[derive(Clone)]
    struct Portal {
        identity: Identity,
        organisations: Organisations,
    }

    impl FromRef<Portal> for Identity {
        fn from_ref(portal: &Portal) -> Self {
            portal.identity.clone()
        }
    }

    impl FromRef<Portal> for Organisations {
        fn from_ref(portal: &Portal) -> Self {
            portal.organisations.clone()
        }
    }

    #[test]
    fn two_contexts_read_their_own_state_out_of_one_surface() {
        let portal = Portal {
            identity: Identity("who a human is"),
            organisations: Organisations("what a business is"),
        };
        assert_eq!(Identity::from_ref(&portal), Identity("who a human is"));
        assert_eq!(
            Organisations::from_ref(&portal),
            Organisations("what a business is")
        );
    }

    // The blanket impl is what keeps every single-context `register_actions` compiling.
    #[test]
    fn a_state_projects_to_itself() {
        let identity = Identity("who a human is");
        assert_eq!(Identity::from_ref(&identity), identity);
    }
}
