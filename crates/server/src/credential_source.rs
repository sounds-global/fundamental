use fundamental_auth::AuthError;
use fundamental_core::secrets::SecretString;

use crate::cookie_lookup::CookieLookup;
use crate::presented_credential::PresentedCredential;

/// Where an HTTP boundary reads the caller's credential from, before verifying it.
///
/// Name a session cookie with the `__Host-` prefix, so a sibling subdomain cannot shadow it.
///
/// ```
/// # extern crate fundamental_server;
/// use fundamental_server::CredentialSource;
///
/// // A browser-only surface.
/// let web = CredentialSource::cookie("__Host-session");
/// // One deployment serving native callers (bearer) and browsers (cookie).
/// let both = CredentialSource::bearer_then_cookie("__Host-session");
/// assert!(web.reads_cookie() && both.reads_cookie());
/// // The default is exactly today's behaviour.
/// assert_eq!(CredentialSource::default(), CredentialSource::BearerHeader);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CredentialSource {
    /// `Authorization: Bearer <token>` only. The default.
    #[default]
    BearerHeader,
    /// A named cookie only, for a surface whose callers are browsers.
    Cookie {
        /// The cookie name, matched case-sensitively.
        name: String,
    },
    /// The bearer header, then the named cookie.
    BearerThenCookie {
        /// The cookie name, matched case-sensitively.
        cookie: String,
    },
}

impl CredentialSource {
    /// Reads the credential from the cookie named `name`, and nowhere else.
    #[must_use]
    pub fn cookie(name: impl Into<String>) -> Self {
        Self::Cookie { name: name.into() }
    }

    /// Reads the bearer header first, then falls back to the cookie named `name`.
    #[must_use]
    pub fn bearer_then_cookie(name: impl Into<String>) -> Self {
        Self::BearerThenCookie {
            cookie: name.into(),
        }
    }

    /// Whether this source reads a cookie, which makes a verifier and a same-origin proof mandatory.
    #[must_use]
    pub fn reads_cookie(&self) -> bool {
        self.cookie_name().is_some()
    }

    /// The configured cookie name, if this source reads one.
    #[must_use]
    pub fn cookie_name(&self) -> Option<&str> {
        match self {
            Self::BearerHeader => None,
            Self::Cookie { name } | Self::BearerThenCookie { cookie: name } => Some(name),
        }
    }

    /// Whether the configured cookie name carries the `__Host-` or `__Secure-` prefix.
    #[must_use]
    pub fn has_prefixed_cookie_name(&self) -> bool {
        self.cookie_name()
            .is_some_and(|name| name.starts_with("__Host-") || name.starts_with("__Secure-"))
    }

    /// Selects the credential this request presented, if any, from every `Cookie` field.
    ///
    /// The first place that yields a credential is the answer, with no fall-through when it
    /// later fails verification.
    ///
    /// # Errors
    /// [`AuthError::AmbiguousCredential`] (`AUTH_009`, `401`) if the configured
    /// cookie name arrived with more than one distinct value.
    pub fn select<'a>(
        &self,
        authorization: Option<&str>,
        cookie_fields: impl Iterator<Item = &'a str>,
    ) -> Result<Option<PresentedCredential>, AuthError> {
        match self {
            Self::BearerHeader => {
                Ok(bearer_token(authorization).map(PresentedCredential::BearerHeader))
            }
            Self::Cookie { name } => cookie_token(cookie_fields, name),
            Self::BearerThenCookie { cookie } => match bearer_token(authorization) {
                Some(token) => Ok(Some(PresentedCredential::BearerHeader(token))),
                None => cookie_token(cookie_fields, cookie),
            },
        }
    }
}

/// Extracts a bearer token from an `Authorization` header value; an empty token is [`None`].
fn bearer_token(authorization: Option<&str>) -> Option<SecretString> {
    let value = authorization?;
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?
        .trim();
    (!token.is_empty()).then(|| SecretString::new(token.to_owned()))
}

/// Reads the named cookie, mapping an ambiguous result to the typed refusal.
fn cookie_token<'a>(
    fields: impl Iterator<Item = &'a str>,
    name: &str,
) -> Result<Option<PresentedCredential>, AuthError> {
    match CookieLookup::find(fields, name) {
        CookieLookup::Absent => Ok(None),
        CookieLookup::Found(value) => Ok(Some(PresentedCredential::Cookie(SecretString::new(
            value.to_owned(),
        )))),
        CookieLookup::Ambiguous(count) => Err(AuthError::AmbiguousCredential {
            name: name.to_owned(),
            count,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    fn select(
        source: &CredentialSource,
        authorization: Option<&str>,
        cookies: &[&str],
    ) -> Result<Option<PresentedCredential>, AuthError> {
        source.select(authorization, cookies.iter().copied())
    }

    #[test]
    fn the_default_is_the_bearer_header() {
        assert_eq!(CredentialSource::default(), CredentialSource::BearerHeader);
        assert!(!CredentialSource::default().reads_cookie());
        assert_eq!(CredentialSource::default().cookie_name(), None);
    }

    #[test]
    fn the_constructors_name_the_cookie() {
        assert_eq!(
            CredentialSource::cookie("__Host-session").cookie_name(),
            Some("__Host-session")
        );
        assert_eq!(
            CredentialSource::bearer_then_cookie("__Host-session").cookie_name(),
            Some("__Host-session")
        );
        assert!(CredentialSource::cookie("s").reads_cookie());
    }

    #[test]
    fn prefixed_names_are_recognised() {
        assert!(CredentialSource::cookie("__Host-session").has_prefixed_cookie_name());
        assert!(CredentialSource::cookie("__Secure-session").has_prefixed_cookie_name());
        assert!(!CredentialSource::cookie("session").has_prefixed_cookie_name());
        assert!(!CredentialSource::BearerHeader.has_prefixed_cookie_name());
    }

    #[test]
    fn the_bearer_source_reads_the_header_and_ignores_cookies() {
        let source = CredentialSource::BearerHeader;
        let found = select(&source, Some("Bearer abc"), &["session=cookie-value"])
            .expect("no ambiguity")
            .expect("a credential");
        assert_eq!(found.token().expose(), "abc");
        assert!(!found.is_cookie());

        assert!(
            select(&source, None, &["session=cookie-value"])
                .expect("no ambiguity")
                .is_none(),
            "a cookie must not authenticate a bearer-only surface"
        );
    }

    #[test]
    fn both_bearer_spellings_are_accepted() {
        for header in ["Bearer abc", "bearer abc", "Bearer   abc  "] {
            let found = select(&CredentialSource::BearerHeader, Some(header), &[])
                .expect("no ambiguity")
                .expect("a credential");
            assert_eq!(found.token().expose(), "abc", "{header}");
        }
    }

    #[test]
    fn another_scheme_is_not_a_bearer() {
        assert!(
            select(&CredentialSource::BearerHeader, Some("Basic abc"), &[])
                .expect("no ambiguity")
                .is_none()
        );
    }

    #[test]
    fn an_empty_bearer_is_absent_not_empty() {
        for header in ["Bearer ", "bearer    "] {
            assert!(
                select(&CredentialSource::BearerHeader, Some(header), &[])
                    .expect("no ambiguity")
                    .is_none(),
                "{header}"
            );
        }
    }

    #[test]
    fn the_cookie_source_reads_the_cookie_and_ignores_the_bearer() {
        let source = CredentialSource::cookie("__Host-session");
        let found = select(&source, Some("Bearer abc"), &["__Host-session=sess"])
            .expect("no ambiguity")
            .expect("a credential");
        assert_eq!(found.token().expose(), "sess");
        assert!(found.is_cookie());

        assert!(
            select(&source, Some("Bearer abc"), &[])
                .expect("no ambiguity")
                .is_none(),
            "a bearer must not authenticate a cookie-only surface"
        );
    }

    #[test]
    fn bearer_then_cookie_prefers_the_header() {
        let source = CredentialSource::bearer_then_cookie("__Host-session");
        let found = select(&source, Some("Bearer abc"), &["__Host-session=sess"])
            .expect("no ambiguity")
            .expect("a credential");
        assert_eq!(found.token().expose(), "abc");
        assert!(!found.is_cookie());
    }

    #[test]
    fn bearer_then_cookie_falls_back_when_the_header_is_absent() {
        let source = CredentialSource::bearer_then_cookie("__Host-session");
        let found = select(&source, None, &["__Host-session=sess"])
            .expect("no ambiguity")
            .expect("a credential");
        assert_eq!(found.token().expose(), "sess");
        assert!(found.is_cookie());
    }

    // A place that later fails does not fall through; that half is pinned at the boundary.
    #[test]
    fn an_empty_bearer_falls_through_to_the_cookie() {
        let source = CredentialSource::bearer_then_cookie("__Host-session");
        let found = select(&source, Some("Bearer "), &["__Host-session=sess"])
            .expect("no ambiguity")
            .expect("a credential");
        assert_eq!(
            found.token().expose(),
            "sess",
            "an empty header must not consume the slot and strand a valid session"
        );
    }

    #[test]
    fn an_ambiguous_cookie_is_refused_with_a_401() {
        let source = CredentialSource::cookie("__Host-session");
        let error = select(
            &source,
            None,
            &["__Host-session=attacker; __Host-session=real"],
        )
        .expect_err("ambiguous");
        assert_eq!(error.error_code(), "AUTH_009");
        assert_eq!(error.http_status_code(), 401);
        assert!(!error.is_retryable());
    }

    #[test]
    fn the_ambiguity_refusal_carries_no_cookie_value() {
        let source = CredentialSource::cookie("__Host-session");
        let error = select(
            &source,
            None,
            &["__Host-session=attacker; __Host-session=real"],
        )
        .expect_err("ambiguous");
        let rendered = error.to_string();
        assert!(rendered.contains("__Host-session"), "{rendered}");
        assert!(!rendered.contains("attacker"), "{rendered}");
        assert!(!rendered.contains("real"), "{rendered}");
    }

    #[test]
    fn a_valid_bearer_wins_before_an_ambiguous_cookie_is_read() {
        let source = CredentialSource::bearer_then_cookie("__Host-session");
        let found = select(
            &source,
            Some("Bearer abc"),
            &["__Host-session=a; __Host-session=b"],
        )
        .expect("the bearer is taken first")
        .expect("a credential");
        assert_eq!(found.token().expose(), "abc");
    }
}
