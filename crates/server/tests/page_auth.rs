//! The gate's decision table, asserted row by row.

use fundamental_core::identity::{AuthenticatedPrincipal, UserId};
use fundamental_server::{
    Audience, HttpContext, LoginRoute, Method, PageAuth, PageVerdict, PathParams, PublicRoute,
    RequestHeaders,
};

const LOGIN: LoginRoute = LoginRoute::new("/login");
const PUBLIC: [PublicRoute; 2] = [PublicRoute::Exact("/"), PublicRoute::Tree("/legal")];

/// A request for `template`, with the headers a browser navigation carries.
fn navigation(template: &str) -> HttpContext<()> {
    let headers = headers(&[("sec-fetch-mode", "navigate")]);
    HttpContext::new((), "test")
        .with_method(Method::Get)
        .with_route_template(template.into())
        .with_headers(headers)
}

/// A request for `template` from an island's `fetch`.
fn fetch(template: &str) -> HttpContext<()> {
    let headers = headers(&[("sec-fetch-mode", "cors")]);
    HttpContext::new((), "test")
        .with_method(Method::Get)
        .with_route_template(template.into())
        .with_headers(headers)
}

fn headers(pairs: &[(&str, &str)]) -> RequestHeaders {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

fn params(pairs: &[(&str, &str)]) -> PathParams {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

fn gate() -> PageAuth {
    PageAuth::new(LOGIN, &PUBLIC)
}

/// The same gate as a native app's page bridge mounts it.
fn native_gate() -> PageAuth {
    PageAuth::for_navigations(LOGIN, &PUBLIC)
}

fn signed_in() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        user_id: UserId::new("staff-1"),
        device_id: None,
        scopes: Vec::new(),
        expires_at: i64::MAX,
    }
}

#[test]
fn a_signed_in_visitor_reaches_a_gated_page() {
    let context = navigation("/clients").with_principal(Some(signed_in()));
    assert_eq!(gate().verdict(&context), PageVerdict::Allow);
}

#[test]
fn a_public_route_is_served_without_a_principal() {
    assert_eq!(gate().verdict(&navigation("/")), PageVerdict::Allow);
}

#[test]
fn a_public_tree_covers_what_is_under_it_and_nothing_beside_it() {
    assert_eq!(
        gate().verdict(&navigation("/legal/dpa")),
        PageVerdict::Allow
    );
    // `/legalese` must not be opened by a `/legal` entry.
    assert!(matches!(
        gate().verdict(&navigation("/legalese")),
        PageVerdict::Redirect(_)
    ));
}

#[test]
fn a_route_the_policy_never_heard_of_is_gated() {
    assert!(matches!(
        gate().verdict(&navigation("/clients/{id}")),
        PageVerdict::Redirect(_)
    ));
}

#[test]
fn the_sign_in_page_is_public_without_being_listed() {
    // `PUBLIC` does not contain `/login`; without adding it on construction every visit would loop.
    assert_eq!(gate().verdict(&navigation("/login")), PageVerdict::Allow);
}

#[test]
fn a_navigation_is_redirected_and_a_fetch_is_refused() {
    assert!(matches!(
        gate().verdict(&navigation("/clients")),
        PageVerdict::Redirect(_)
    ));
    assert_eq!(gate().verdict(&fetch("/clients")), PageVerdict::Refuse);
}

#[test]
fn with_no_sec_fetch_header_only_an_explicit_html_accept_reads_as_a_person() {
    let bare = |accept: Option<&str>| {
        let carried = accept.map_or_else(Vec::new, |accept| vec![("accept", accept)]);
        HttpContext::new((), "test")
            .with_method(Method::Get)
            .with_route_template("/clients".into())
            .with_headers(headers(&carried))
    };
    assert_eq!(
        Audience::of(&bare(Some("text/html,application/xhtml+xml"))),
        Audience::Navigation
    );
    assert_eq!(
        Audience::of(&bare(Some("application/json"))),
        Audience::Programmatic
    );
    // `*/*` is what curl sends.
    assert_eq!(Audience::of(&bare(Some("*/*"))), Audience::Programmatic);
    assert_eq!(Audience::of(&bare(None)), Audience::Programmatic);
}

// A `303` to a URL that only answered a `POST` would land on a `405`.
#[test]
fn a_post_is_redirected_without_a_return_path() {
    let context = HttpContext::new((), "test")
        .with_method(Method::Post)
        .with_route_template("/clients".into())
        .with_headers(headers(&[("sec-fetch-mode", "navigate")]));
    assert_eq!(gate().verdict(&context), PageVerdict::Redirect(None));
}

#[test]
fn a_context_with_no_matched_route_is_gated() {
    let context = HttpContext::new((), "test")
        .with_method(Method::Get)
        .with_headers(headers(&[("sec-fetch-mode", "navigate")]));
    assert!(matches!(gate().verdict(&context), PageVerdict::Redirect(_)));
}

#[test]
fn the_return_path_is_rebuilt_from_the_matched_template() {
    let context = navigation("/clients/{id}")
        .with_params(params(&[("id", "42")]))
        .with_query("tab=files");
    let PageVerdict::Redirect(Some(path)) = gate().verdict(&context) else {
        panic!("a gated navigation must carry where it was going");
    };
    assert_eq!(path.as_location(), "/clients/42?tab=files");
    assert_eq!(
        LOGIN.location(Some(&path), None),
        "/login?next=%2Fclients%2F42%3Ftab%3Dfiles"
    );
}

#[test]
fn the_sign_in_redirect_keeps_the_market_and_next_is_the_served_address() {
    use fundamental_core::address::{
        AddressSpace, LocalePrefixSet, PageSlug, PrefixScope, RequestAddress, SlugTable,
    };
    use std::sync::Arc;

    let prefixes = LocalePrefixSet::new()
        .with_paths_in(
            "nl",
            "nl-NL".parse().expect("a locale"),
            PrefixScope::Full,
            "nl",
        )
        .expect("one prefix");
    let slugs = SlugTable::builder()
        .page("/login", &[PageSlug::current("nl", "inloggen")])
        .page("/clients/{id}", &[])
        .build()
        .expect("a well-formed table");
    let space = Arc::new(AddressSpace::new(prefixes, slugs));
    let address = RequestAddress::new(
        space,
        "nl-NL".parse().expect("a locale"),
        "/nl",
        "/clients/{id}",
    );

    let context = navigation("/clients/{id}")
        .with_params(params(&[("id", "42")]))
        .with_query("tab=a%2fb");
    let PageVerdict::Redirect(Some(path)) = gate().verdict(&context) else {
        panic!("a gated navigation must carry where it was going");
    };

    assert_eq!(
        LOGIN.location(Some(&path), Some(&address)),
        "/nl/inloggen?next=%2Fnl%2Fclients%2F42%3Ftab%3Da%252fb",
        "the reader returns where they were served, the query byte for byte"
    );
}

#[test]
fn a_captured_parameter_cannot_restructure_the_url() {
    for hostile in ["../../etc", "x?y=z", "//evil.test", "a b"] {
        let context = navigation("/clients/{id}").with_params(params(&[("id", hostile)]));
        let PageVerdict::Redirect(Some(path)) = gate().verdict(&context) else {
            panic!("expected a redirect for {hostile}");
        };
        let location = path.as_location();
        assert!(
            location.starts_with("/clients/") && !location[9..].contains('/'),
            "`{hostile}` escaped its segment: {location}"
        );
    }
}

// The page bridge synthesises no headers, so the request carries nothing: the worst case.
#[test]
fn a_native_launch_reaches_a_door_even_when_the_request_says_nothing() {
    let bare = HttpContext::new((), "test").with_route_template("/clients".into());
    assert_eq!(
        gate().verdict(&bare),
        PageVerdict::Refuse,
        "the ordinary gate fails closed on a headerless request, which is what makes the          native one a decision rather than a default"
    );
    assert!(matches!(
        native_gate().verdict(&bare),
        PageVerdict::Redirect(_)
    ));
}

// Declaring the audience changes what an anonymous caller is answered with, never what they reach.
#[test]
fn declaring_the_audience_changes_no_other_answer() {
    assert_eq!(
        native_gate().verdict(&navigation("/clients").with_principal(Some(signed_in()))),
        PageVerdict::Allow
    );
    assert_eq!(native_gate().verdict(&navigation("/")), PageVerdict::Allow);
    assert_eq!(
        native_gate().verdict(&navigation("/login")),
        PageVerdict::Allow
    );
    assert!(matches!(
        native_gate().verdict(&fetch("/clients")),
        PageVerdict::Redirect(_)
    ));
}
