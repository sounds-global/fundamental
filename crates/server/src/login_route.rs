use fundamental_core::address::RequestAddress;

use crate::{PublicRoute, ReturnPath};

/// Where an unauthenticated navigation is sent; [`PageAuth`] makes it public, so it cannot gate itself.
///
/// [`PageAuth`]: crate::PageAuth
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoginRoute(&'static str);

impl LoginRoute {
    /// The route the sign-in page is served at.
    #[must_use]
    pub const fn new(template: &'static str) -> Self {
        Self(template)
    }

    /// The template, so it can be added to the public list.
    #[must_use]
    pub const fn template(self) -> &'static str {
        self.0
    }

    /// This route as a public entry.
    #[must_use]
    pub const fn as_public(self) -> PublicRoute {
        PublicRoute::Exact(self.0)
    }

    /// The `Location` for a person who was going to `return_to`, in the market they were in.
    ///
    /// `next` is rebuilt from the route template and spelled through the address space, so an
    /// open redirect is unrepresentable.
    #[must_use]
    pub fn location(
        self,
        return_to: Option<&ReturnPath>,
        address: Option<&RequestAddress>,
    ) -> String {
        // An unaddressed surface and an unknown prefix are one branch.
        let at = address.map_or_else(
            || self.0.to_owned(),
            |address| address.space().address(address.prefix(), self.0),
        );
        // The home page is where a sign-in lands anyway, so it needs no `next`.
        match return_to {
            Some(path) if path.as_location() != "/" => {
                let served =
                    address.map_or_else(|| path.clone(), |address| path.served_at(address));
                format!("{at}?next={}", served.encoded())
            }
            _ => at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_page_is_left_out_and_any_other_page_is_kept() {
        let login = LoginRoute::new("/login");
        let home = ReturnPath::parse("/").expect("valid");
        let page = ReturnPath::parse("/clients?page=2").expect("valid");
        assert_eq!(login.location(Some(&home), None), "/login");
        assert_eq!(login.location(None, None), "/login");
        assert_eq!(
            login.location(Some(&page), None),
            "/login?next=%2Fclients%3Fpage%3D2"
        );
    }
}
