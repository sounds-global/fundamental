use tracing::warn;

use crate::{FetchSite, Method, ObservedOrigin, RequestHeaders, ServerError};

/// Refuses a safe request to a cross-origin route that a page outside the route's origin set provoked.
///
/// Passes with no `Sec-Fetch-Site`, `same-origin` or `none`; otherwise only an `Origin` in the
/// route's set passes. An unsafe method is left to the write guard.
///
/// # Errors
/// [`ServerError::CrossSiteRead`] (`API_031`, `403`) when another site's page provoked it.
pub fn guard_cross_origin_read(
    method: Method,
    headers: &RequestHeaders,
    origins: &[String],
) -> Result<(), ServerError> {
    if !method.is_safe() {
        return Ok(());
    }
    let site = FetchSite::parse(headers.get("sec-fetch-site"));
    if matches!(
        site,
        FetchSite::Absent | FetchSite::SameOrigin | FetchSite::None
    ) {
        return Ok(());
    }
    let origin = headers.get("origin").filter(|value| *value != "null");
    if origin.is_some_and(|value| origins.iter().any(|allowed| allowed == value)) {
        return Ok(());
    }
    let origin = ObservedOrigin::parse(headers.get("origin"));
    warn!(
        method = method.as_str(),
        sec_fetch_site = site.as_str(),
        origin = %origin,
        "refused a read on a cross-origin route that another site's page provoked"
    );
    Err(ServerError::CrossSiteRead { site, origin })
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> RequestHeaders {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn site() -> Vec<String> {
        vec!["https://site.test".to_owned()]
    }

    #[test]
    fn an_image_on_another_page_is_refused() {
        let refused = guard_cross_origin_read(
            Method::Get,
            &headers(&[("sec-fetch-site", "cross-site")]),
            &site(),
        )
        .unwrap_err();
        assert_eq!(refused.error_code(), "API_031");
        assert_eq!(refused.http_status_code(), 403);
    }

    #[test]
    fn the_site_s_own_fetch_passes() {
        let fetch = headers(&[
            ("sec-fetch-site", "same-site"),
            ("origin", "https://site.test"),
        ]);
        assert!(guard_cross_origin_read(Method::Get, &fetch, &site()).is_ok());
    }

    #[test]
    fn an_unrecognised_signal_needs_the_origin() {
        let odd = headers(&[("sec-fetch-site", "elsewhere")]);
        assert!(guard_cross_origin_read(Method::Get, &odd, &site()).is_err());
    }

    #[test]
    fn a_write_is_left_to_the_write_guard() {
        let post = headers(&[("sec-fetch-site", "cross-site")]);
        assert!(guard_cross_origin_read(Method::Post, &post, &site()).is_ok());
    }
}
