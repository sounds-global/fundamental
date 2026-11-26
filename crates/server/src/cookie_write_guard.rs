use tracing::warn;

use crate::{FetchSite, Method, ObservedOrigin, RequestHeaders, ServerError};

/// Refuses an unsafe request whose credential came from a cookie unless the browser proves it same-origin.
///
/// A safe method is never guarded. `Sec-Fetch-Site` decides when present (an origin in
/// `trusted_origins` still passes); otherwise `Origin` must match `authority` or be trusted;
/// otherwise the request is refused. The opaque origin `null` never matches.
///
/// # Errors
/// [`ServerError::CrossSiteWrite`] (`API_015`, `403`) when the request cannot be
/// shown to be same-origin.
pub fn guard_cookie_write(
    method: Method,
    headers: &RequestHeaders,
    authority: Option<&str>,
    trusted_origins: &[String],
) -> Result<(), ServerError> {
    if method.is_safe() {
        return Ok(());
    }

    // The opaque origin `null` identifies nobody, even if someone lists it.
    let origin = headers.get("origin").filter(|value| *value != "null");
    let explicitly_trusted =
        origin.is_some_and(|value| trusted_origins.iter().any(|trusted| trusted == value));

    // Without the trusted-origin exception the allow-list would be unreachable in every modern browser.
    let site = FetchSite::parse(headers.get("sec-fetch-site"));
    if site != FetchSite::Absent {
        return match site {
            FetchSite::SameOrigin | FetchSite::None => Ok(()),
            _ if explicitly_trusted => Ok(()),
            _ => Err(refuse(method, site, headers)),
        };
    }

    let Some(origin) = origin else {
        return Err(refuse(method, site, headers));
    };
    if explicitly_trusted
        || authority.is_some_and(|addressed| origin_authority(origin) == Some(addressed))
    {
        return Ok(());
    }
    Err(refuse(method, site, headers))
}

/// Builds the refusal and logs both signals as classes or a parsed origin, never as sent.
fn refuse(method: Method, site: FetchSite, headers: &RequestHeaders) -> ServerError {
    let origin = ObservedOrigin::parse(headers.get("origin"));
    warn!(
        method = method.as_str(),
        sec_fetch_site = site.as_str(),
        origin = %origin,
        "refused a cookie-authenticated write that could not be proved same-origin"
    );
    ServerError::CrossSiteWrite { site, origin }
}

/// The `host[:port]` half of an `Origin` value; [`None`] with no scheme separator.
fn origin_authority(origin: &str) -> Option<&str> {
    origin.split_once("://").map(|(_, authority)| authority)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    fn headers(pairs: &[(&str, &str)]) -> RequestHeaders {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    /// The common shape: a caller that addressed `app.example.com`.
    fn guard(h: &RequestHeaders, trusted: &[String]) -> Result<(), ServerError> {
        guard_cookie_write(Method::Post, h, Some("app.example.com"), trusted)
    }

    const UNSAFE_METHODS: [Method; 4] = [Method::Post, Method::Put, Method::Patch, Method::Delete];

    #[test]
    fn a_safe_method_is_never_guarded() {
        let hostile = headers(&[("sec-fetch-site", "cross-site")]);
        assert!(guard_cookie_write(Method::Get, &hostile, Some("app.example.com"), &[]).is_ok());
    }

    #[test]
    fn same_origin_sec_fetch_site_passes() {
        let h = headers(&[("sec-fetch-site", "same-origin")]);
        for method in UNSAFE_METHODS {
            assert!(
                guard_cookie_write(method, &h, Some("app.example.com"), &[]).is_ok(),
                "{method}"
            );
        }
    }

    /// `none` is a direct navigation or a user gesture — no other site is involved.
    #[test]
    fn none_sec_fetch_site_passes() {
        assert!(guard(&headers(&[("sec-fetch-site", "none")]), &[]).is_ok());
    }

    // `same-site` is the sibling-subdomain position a cookie-tossing attacker occupies.
    #[test]
    fn same_site_and_cross_site_are_refused() {
        for site in ["same-site", "cross-site"] {
            let error = guard(&headers(&[("sec-fetch-site", site)]), &[]).expect_err(site);
            assert_eq!(error.error_code(), "API_015");
            assert_eq!(error.http_status_code(), 403);
            assert!(!error.is_retryable());
        }
    }

    /// The browser's signal wins over `Origin`, because script cannot set it.
    #[test]
    fn sec_fetch_site_decides_even_when_origin_looks_fine() {
        let h = headers(&[
            ("sec-fetch-site", "cross-site"),
            ("origin", "https://app.example.com"),
            ("host", "app.example.com"),
        ]);
        assert!(guard(&h, &[]).is_err());
    }

    #[test]
    fn an_origin_matching_the_addressed_authority_passes() {
        let h = headers(&[("origin", "https://app.example.com")]);
        assert!(guard(&h, &[]).is_ok());
    }

    #[test]
    fn a_port_is_part_of_the_authority() {
        let h = headers(&[("origin", "http://127.0.0.1:9620")]);
        assert!(guard_cookie_write(Method::Post, &h, Some("127.0.0.1:9620"), &[]).is_ok());
        assert!(guard_cookie_write(Method::Post, &h, Some("127.0.0.1:9621"), &[]).is_err());
    }

    #[test]
    fn a_foreign_origin_is_refused() {
        let error =
            guard(&headers(&[("origin", "https://evil.test")]), &[]).expect_err("cross-site");
        assert_eq!(error.error_code(), "API_015");
    }

    #[test]
    fn a_sibling_subdomain_is_refused() {
        assert!(guard(&headers(&[("origin", "https://evil.example.com")]), &[]).is_err());
    }

    #[test]
    fn an_explicitly_trusted_origin_passes() {
        let h = headers(&[("origin", "https://partner.test")]);
        assert!(guard(&h, &["https://partner.test".to_owned()]).is_ok());
        assert!(
            guard(&h, &[]).is_err(),
            "trust must be configured, never inferred"
        );
    }

    #[test]
    fn an_explicitly_trusted_origin_passes_even_when_sec_fetch_site_says_cross_site() {
        let h = headers(&[
            ("sec-fetch-site", "cross-site"),
            ("origin", "https://partner.test"),
        ]);
        assert!(guard(&h, &["https://partner.test".to_owned()]).is_ok());
        assert!(guard(&h, &[]).is_err());
    }

    /// The opaque origin proves nothing about the caller.
    #[test]
    fn the_null_origin_is_refused() {
        let h = headers(&[("origin", "null")]);
        assert!(guard(&h, &[]).is_err());
        assert!(
            guard(&h, &["null".to_owned()]).is_err(),
            "even configuring it must not make the opaque origin trusted"
        );
        let with_signal = headers(&[("sec-fetch-site", "cross-site"), ("origin", "null")]);
        assert!(guard(&with_signal, &["null".to_owned()]).is_err());
    }

    /// Fail closed: a request that presented no signal has proved nothing.
    #[test]
    fn neither_signal_is_refused() {
        let error = guard(&headers(&[]), &[]).expect_err("no signal");
        assert_eq!(error.error_code(), "API_015");
        assert!(error.to_string().contains("absent"));
    }

    #[test]
    fn an_origin_with_no_scheme_cannot_match_an_authority() {
        assert!(guard(&headers(&[("origin", "app.example.com")]), &[]).is_err());
    }

    #[test]
    fn the_authority_comes_from_the_caller_not_from_a_host_header() {
        let h = headers(&[("origin", "https://app.example.com")]);
        assert!(
            guard_cookie_write(Method::Post, &h, Some("app.example.com"), &[]).is_ok(),
            "no `Host` header present, and it must not be needed"
        );
        assert!(
            guard_cookie_write(Method::Post, &h, None, &[]).is_err(),
            "with no authority to compare against, fail closed"
        );
    }

    #[test]
    fn a_refusal_carries_classes_and_a_parsed_origin_never_the_headers() {
        let hostile = "jan@example.nl\u{1b}[2J".repeat(500);
        let cases = [
            (
                headers(&[("sec-fetch-site", hostile.as_str())]),
                FetchSite::Unrecognised,
                ObservedOrigin::Absent,
            ),
            (
                headers(&[("origin", hostile.as_str())]),
                FetchSite::Absent,
                ObservedOrigin::Malformed,
            ),
            (
                headers(&[
                    ("sec-fetch-site", "cross-site"),
                    ("origin", "https://evil.test:8443"),
                ]),
                FetchSite::CrossSite,
                ObservedOrigin::parse(Some("https://evil.test:8443")),
            ),
        ];
        for (h, expected_site, expected_origin) in cases {
            match guard(&h, &[]) {
                Err(ServerError::CrossSiteWrite { site, origin }) => {
                    assert_eq!(site, expected_site);
                    assert_eq!(origin, expected_origin);
                    let rendered = format!("{site} {origin}");
                    assert!(!rendered.contains("jan"), "{rendered}");
                }
                other => panic!("expected a cross-site refusal, got {other:?}"),
            }
        }
    }
}
