use crate::{ServerError, ServerResult};

/// The longest `Location` this accepts: the URL length every mainstream browser and proxy honours.
const MAX_LOCATION: usize = 2048;

/// A redirect: a real 3xx status and a `Location` validated to survive the wire.
///
/// Whether the target is one to send anybody to is the application's decision, not this type's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    status: u16,
    location: String,
}

impl Redirect {
    /// `303 See Other`, the answer to a successful `POST`.
    ///
    /// # Errors
    /// [`ServerError::InvalidRedirect`] (`API_016`) — see `Self::to`.
    pub fn see_other(location: impl Into<String>) -> ServerResult<Self> {
        Self::to(303, location)
    }

    /// `307 Temporary Redirect` — go elsewhere for this request, keeping the method.
    ///
    /// # Errors
    /// [`ServerError::InvalidRedirect`] (`API_016`) — see `Self::to`.
    pub fn temporary(location: impl Into<String>) -> ServerResult<Self> {
        Self::to(307, location)
    }

    /// `308 Permanent Redirect` — this address has moved, keeping the method.
    ///
    /// # Errors
    /// [`ServerError::InvalidRedirect`] (`API_016`) — see `Self::to`.
    pub fn permanent(location: impl Into<String>) -> ServerResult<Self> {
        Self::to(308, location)
    }

    /// `302 Found`, for a protocol that names it; prefer [`see_other`](Self::see_other) or [`temporary`](Self::temporary).
    ///
    /// # Errors
    /// [`ServerError::InvalidRedirect`] (`API_016`) — see `Self::to`.
    pub fn found(location: impl Into<String>) -> ServerResult<Self> {
        Self::to(302, location)
    }

    /// `301 Moved Permanently`, for a protocol that names it; prefer [`permanent`](Self::permanent).
    ///
    /// # Errors
    /// [`ServerError::InvalidRedirect`] (`API_016`) — see `Self::to`.
    pub fn moved_permanently(location: impl Into<String>) -> ServerResult<Self> {
        Self::to(301, location)
    }

    /// The validating constructor; the location is checked as given, before anything trims it.
    fn to(status: u16, location: impl Into<String>) -> ServerResult<Self> {
        let location = location.into();
        if location.is_empty() {
            return Err(ServerError::InvalidRedirect {
                reason: "the location is empty",
            });
        }
        if location.len() > MAX_LOCATION {
            return Err(ServerError::InvalidRedirect {
                reason: "the location is longer than a redirect target may be",
            });
        }
        // Visible ASCII, no space: anything else cannot be a header value or a URI reference.
        if !location.bytes().all(|byte| (0x21..=0x7e).contains(&byte)) {
            return Err(ServerError::InvalidRedirect {
                reason: "the location holds a character a URI reference cannot carry; \
                         percent-encode it before redirecting",
            });
        }
        Ok(Self { status, location })
    }

    /// The 3xx status this redirect carries.
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// Where it points.
    #[must_use]
    pub fn location(&self) -> &str {
        &self.location
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_LOCATION, Redirect};
    use fundamental_core::Error;

    #[test]
    fn each_constructor_names_its_own_status() {
        assert_eq!(Redirect::see_other("/a").expect("valid").status(), 303);
        assert_eq!(Redirect::temporary("/a").expect("valid").status(), 307);
        assert_eq!(Redirect::permanent("/a").expect("valid").status(), 308);
        assert_eq!(Redirect::found("/a").expect("valid").status(), 302);
        assert_eq!(
            Redirect::moved_permanently("/a").expect("valid").status(),
            301
        );
    }

    #[test]
    fn an_ordinary_target_survives_unchanged() {
        let redirect = Redirect::see_other("/login?next=%2Fclients%3Fpage%3D2").expect("valid");
        assert_eq!(redirect.location(), "/login?next=%2Fclients%3Fpage%3D2");
    }

    #[test]
    fn another_origin_is_allowed_because_a_handoff_needs_it() {
        // The framework does not decide whose origins are acceptable; an OAuth handoff relies on it.
        let redirect = Redirect::temporary("https://issuer.example/authorize?x=1").expect("valid");
        assert_eq!(redirect.location(), "https://issuer.example/authorize?x=1");
    }

    #[test]
    fn a_location_carrying_a_line_break_is_refused_rather_than_trimmed() {
        for hostile in [
            "/login\r\nSet-Cookie: a=b",
            "/login\n",
            "/login\r",
            "/login\u{0}",
        ] {
            let refusal = Redirect::see_other(hostile).expect_err("must be refused");
            assert_eq!(refusal.error_code(), "API_016");
        }
    }

    #[test]
    fn a_location_that_is_not_a_uri_reference_is_refused() {
        for unusable in ["/zoek naar dit", "/caf\u{e9}", "/\u{7f}"] {
            assert_eq!(
                Redirect::see_other(unusable)
                    .expect_err("must be refused")
                    .error_code(),
                "API_016"
            );
        }
    }

    #[test]
    fn an_empty_or_overlong_location_is_refused() {
        assert_eq!(
            Redirect::see_other("")
                .expect_err("must be refused")
                .error_code(),
            "API_016"
        );
        let overlong = format!("/{}", "a".repeat(MAX_LOCATION));
        assert_eq!(
            Redirect::see_other(overlong)
                .expect_err("must be refused")
                .error_code(),
            "API_016"
        );
    }

    #[test]
    fn the_longest_permitted_location_is_accepted() {
        let longest = format!("/{}", "a".repeat(MAX_LOCATION - 1));
        assert_eq!(
            Redirect::see_other(&longest).expect("valid").location(),
            longest
        );
    }
}
