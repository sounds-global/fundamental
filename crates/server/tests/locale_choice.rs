//! The language recorder's decision table, asserted row by row, and once through a real route.

use fundamental_server::{
    HttpContext, LanguageCookie, LanguageVerdict, Locale, LocaleChoice, LocalePolicy, LocaleSource,
    Method, PathParams, RequestHeaders, Route,
};

/// English source, Dutch offered, cookie first and then `Accept-Language`.
fn policy() -> LocalePolicy {
    LocalePolicy::new(locale("en"))
        .with_supported(vec![locale("en"), locale("nl")])
        .with_source(LocaleSource::cookie_then_accept_language("app-locale"))
}

fn locale(tag: &str) -> Locale {
    tag.parse().expect("a language tag")
}

fn recorder() -> LocaleChoice {
    LocaleChoice::of(&policy(), false).expect("the policy reads a cookie")
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

/// A browser navigation to `template`, carrying `query`.
fn navigation(template: &str, query: &str) -> HttpContext<()> {
    HttpContext::new((), "test")
        .with_method(Method::Get)
        .with_route_template(template.into())
        .with_query(query.to_owned())
        .with_headers(headers(&[("sec-fetch-mode", "navigate")]))
}

/// The same request from an island's `fetch`.
fn fetch(template: &str, query: &str) -> HttpContext<()> {
    navigation(template, query).with_headers(headers(&[("sec-fetch-mode", "cors")]))
}

/// Where a `Record` verdict sends the visitor.
#[track_caller]
fn recorded_location(context: &HttpContext<()>) -> String {
    match recorder().verdict(context) {
        LanguageVerdict::Record { return_to, .. } => return_to.as_location().to_owned(),
        other => panic!("expected a recorded choice, got {other:?}"),
    }
}

#[test]
fn a_request_carrying_no_choice_is_untouched() {
    assert_eq!(
        recorder().verdict(&navigation("/why", "")),
        LanguageVerdict::Untouched
    );
    assert_eq!(
        recorder().verdict(&navigation("/why", "utm_source=ads&utm_campaign=wwft")),
        LanguageVerdict::Untouched
    );
}

#[test]
fn a_navigation_choosing_an_offered_language_is_recorded() {
    match recorder().verdict(&navigation("/why", "lang=nl")) {
        LanguageVerdict::Record {
            language,
            return_to,
        } => {
            assert_eq!(language, locale("nl"));
            // Left in, the parameter would make the target ask again.
            assert_eq!(return_to.as_location(), "/why");
        }
        other => panic!("expected a recorded choice, got {other:?}"),
    }
}

#[test]
fn every_other_parameter_survives_the_switch_in_order() {
    // Losing `next` sends a person who switched language to the wrong page after signing in.
    assert_eq!(
        recorded_location(&navigation("/why", "a=1&lang=nl&b=2")),
        "/why?a=1&b=2"
    );
    assert_eq!(
        recorded_location(&navigation("/login", "next=%2Fclients&lang=nl")),
        "/login?next=%2Fclients"
    );
}

#[test]
fn the_only_parameter_leaves_no_trailing_question_mark() {
    // `/why?` is a second address for one page.
    let location = recorded_location(&navigation("/why", "lang=nl"));
    assert!(!location.contains('?'), "location: {location}");
}

#[test]
fn a_parameter_that_merely_starts_with_lang_is_not_a_choice() {
    assert_eq!(
        recorder().verdict(&navigation("/why", "language=nl")),
        LanguageVerdict::Untouched
    );
}

#[test]
fn a_language_this_surface_does_not_serve_is_served_not_refused() {
    // A stale bookmark must not get a redirect loop or a `400`.
    assert_eq!(
        recorder().verdict(&navigation("/why", "lang=zz")),
        LanguageVerdict::Untouched
    );
    assert_eq!(
        recorder().verdict(&navigation("/why", "lang=")),
        LanguageVerdict::Untouched
    );
    assert_eq!(
        recorder().verdict(&navigation("/why", "lang=%00")),
        LanguageVerdict::Untouched
    );
}

#[test]
fn a_regional_variant_is_not_widened_to_the_language_it_belongs_to() {
    // A recorder that guesses writes down something the visitor did not choose.
    assert_eq!(
        recorder().verdict(&navigation("/why", "lang=nl-BE")),
        LanguageVerdict::Untouched
    );
}

#[test]
fn the_tag_is_matched_without_regard_to_case() {
    match recorder().verdict(&navigation("/why", "lang=NL")) {
        LanguageVerdict::Record { language, .. } => assert_eq!(language, locale("nl")),
        other => panic!("expected a recorded choice, got {other:?}"),
    }
}

#[test]
fn two_choices_in_one_request_are_no_choice() {
    assert_eq!(
        recorder().verdict(&navigation("/why", "lang=nl&lang=en")),
        LanguageVerdict::Untouched
    );
}

#[test]
fn a_write_is_never_turned_into_a_redirect() {
    let post = navigation("/why", "lang=nl").with_method(Method::Post);
    assert_eq!(recorder().verdict(&post), LanguageVerdict::Untouched);
}

#[test]
fn a_programmatic_caller_gets_its_data_not_a_door() {
    // `Ignored` rather than `Untouched`: the answer still varies by caller.
    assert_eq!(
        recorder().verdict(&fetch("/why", "lang=nl")),
        LanguageVerdict::Ignored
    );
}

#[test]
fn a_request_the_router_could_not_place_records_nothing() {
    let unmatched = HttpContext::new((), "test")
        .with_method(Method::Get)
        .with_query("lang=nl".to_owned())
        .with_headers(headers(&[("sec-fetch-mode", "navigate")]));
    assert_eq!(recorder().verdict(&unmatched), LanguageVerdict::Untouched);
}

#[test]
fn a_hostile_path_parameter_stays_inside_its_own_segment() {
    let context =
        navigation("/clients/{id}", "lang=nl").with_params(params(&[("id", "../../etc")]));
    let location = recorded_location(&context);
    assert_eq!(location, "/clients/..%2F..%2Fetc");
    assert!(!location.contains("/../"), "location: {location}");
}

#[test]
fn the_offered_set_is_the_policys_own() {
    // The set a link may name and the set the boundary negotiates are one set.
    assert_eq!(recorder().offered(), policy().supported());
}

#[test]
fn a_surface_that_reads_no_cookie_has_nowhere_to_record_a_choice() {
    let accept_language_only = LocalePolicy::new(locale("en")).with_supported(vec![locale("nl")]);
    assert!(LocaleChoice::of(&accept_language_only, false).is_none());
    assert!(LanguageCookie::of(&accept_language_only).is_none());
}

#[test]
fn the_cookie_carries_every_attribute_the_choice_needs() {
    let cookie = LanguageCookie::of(&policy()).expect("the policy reads a cookie");
    let set = cookie.set(&locale("nl"));
    assert_eq!(
        set,
        "app-locale=nl; Path=/; Max-Age=31536000; HttpOnly; SameSite=Lax; Secure"
    );
    // The name is the policy's: a second literal could write one name while the boundary reads another.
    assert_eq!(cookie.name(), "app-locale");
}

#[test]
fn development_drops_secure_and_nothing_else() {
    // A browser silently discards a `Secure` cookie over plain http.
    let production = LanguageCookie::of(&policy())
        .expect("a cookie")
        .for_development(false)
        .set(&locale("nl"));
    let development = LanguageCookie::of(&policy())
        .expect("a cookie")
        .for_development(true)
        .set(&locale("nl"));
    assert_eq!(production, format!("{development}; Secure"));
    assert!(development.contains("HttpOnly") && development.contains("SameSite=Lax"));
}

#[tokio::test]
async fn the_middleware_answers_a_redirect_and_never_runs_the_page() {
    // The handler does not run, and the status and headers reach the sinks the router drains.
    let route = Route::html("/why", |_ctx: HttpContext<()>| async move {
        panic!("the page must not render: the request was answered by the redirect");
    })
    .middleware(recorder());

    let context = navigation("/why", "next=x&lang=nl");
    let status = context.response_status();
    let headers = context.response_headers();
    let redirect = context.response_redirect();

    let body = route.dispatch(context, Vec::new()).await.expect("no error");

    assert!(body.is_empty(), "the short-circuit returns no body");
    assert_eq!(status.take(), Some(303));

    let written = headers.take();
    let value = |name: &str| {
        written
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
    };
    assert_eq!(
        redirect
            .take()
            .map(|redirect| redirect.location().to_owned())
            .as_deref(),
        Some("/why?next=x"),
        "the router writes the Location from the recorded redirect"
    );
    assert_eq!(
        value("set-cookie").as_deref(),
        Some("app-locale=nl; Path=/; Max-Age=31536000; HttpOnly; SameSite=Lax; Secure")
    );
    // The response is a `Set-Cookie`; a shared cache must not replay it.
    assert_eq!(value("cache-control").as_deref(), Some("no-store"));
    // `Vary: Accept-Language, Cookie` is the boundary's, stamped from the policy.
    assert!(
        !written
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("vary")),
        "the recorder must not restate the policy's own Vary: {written:?}"
    );
}

#[tokio::test]
async fn a_request_with_no_choice_reaches_the_page_untouched() {
    let route = Route::html("/why", |_ctx: HttpContext<()>| async move {
        Ok("<h1>Why</h1>".to_owned())
    })
    .middleware(recorder());

    let context = navigation("/why", "utm_source=ads");
    let status = context.response_status();
    let headers = context.response_headers();

    let body = route.dispatch(context, Vec::new()).await.expect("no error");

    assert_eq!(body, b"<h1>Why</h1>");
    assert_eq!(status.take(), None);
    assert!(headers.take().is_empty());
}

#[tokio::test]
async fn an_islands_fetch_reaches_the_page_and_the_answer_says_it_varies() {
    let route = Route::html("/why", |_ctx: HttpContext<()>| async move {
        Ok("<h1>Why</h1>".to_owned())
    })
    .middleware(recorder());

    let context = fetch("/why", "lang=nl");
    let headers = context.response_headers();

    let body = route.dispatch(context, Vec::new()).await.expect("no error");

    assert_eq!(body, b"<h1>Why</h1>");
    // Otherwise a shared cache could replay an island's `200` to a navigation.
    assert!(
        headers
            .take()
            .iter()
            .any(|(key, value)| key.eq_ignore_ascii_case("vary") && value == "sec-fetch-mode")
    );
}

/// The same navigation, made from a link on another site.
fn cross_site(template: &str, query: &str) -> HttpContext<()> {
    navigation(template, query).with_headers(headers(&[
        ("sec-fetch-mode", "navigate"),
        ("sec-fetch-site", "cross-site"),
    ]))
}

#[test]
fn a_cross_site_language_link_sets_no_cookie() {
    match recorder().verdict(&cross_site("/why", "lang=nl&tab=x")) {
        LanguageVerdict::Declined { return_to } => {
            assert_eq!(return_to.as_location(), "/why?tab=x");
        }
        other => panic!("another site's link must not record a choice, got {other:?}"),
    }
}

#[test]
fn the_readers_own_navigation_records_the_choice() {
    for site in ["same-origin", "same-site", "none"] {
        let context = navigation("/why", "lang=nl").with_headers(headers(&[
            ("sec-fetch-mode", "navigate"),
            ("sec-fetch-site", site),
        ]));
        assert!(
            matches!(recorder().verdict(&context), LanguageVerdict::Record { .. }),
            "`{site}` is the reader's own navigation"
        );
    }
}

#[test]
fn an_unrecognised_fetch_site_is_declined() {
    let context = navigation("/why", "lang=nl").with_headers(headers(&[
        ("sec-fetch-mode", "navigate"),
        ("sec-fetch-site", "somewhere-else"),
    ]));
    assert!(matches!(
        recorder().verdict(&context),
        LanguageVerdict::Declined { .. }
    ));
}

#[tokio::test]
async fn a_declined_choice_redirects_to_the_clean_address_without_a_cookie() {
    let route = Route::html("/why", |_ctx: HttpContext<()>| async move {
        panic!("the page must not render: the request was answered by the redirect");
    })
    .middleware(recorder());

    let context = cross_site("/why", "lang=nl");
    let status = context.response_status();
    let headers = context.response_headers();
    let redirect = context.response_redirect();

    let body = route.dispatch(context, Vec::new()).await.expect("no error");

    assert!(body.is_empty());
    assert_eq!(status.take(), Some(303));
    let written = headers.take();
    assert_eq!(
        redirect
            .take()
            .map(|redirect| redirect.location().to_owned())
            .as_deref(),
        Some("/why")
    );
    assert!(
        !written
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("set-cookie")),
        "a cross-site link must not write the cookie: {written:?}"
    );
}

/// English at the bare addresses and Dutch under `/nl`, each page in its own language's words.
fn space() -> std::sync::Arc<fundamental_core::address::AddressSpace> {
    use fundamental_core::address::{AddressSpace, LocalePrefixSet, PageSlug, SlugTable};
    let prefixes = LocalePrefixSet::new()
        .with("nl", locale("nl"))
        .expect("a prefix");
    let slugs = SlugTable::builder()
        .page("/why", &[PageSlug::current("nl", "waarom")])
        .build()
        .expect("well formed");
    std::sync::Arc::new(
        AddressSpace::new(prefixes, slugs)
            .with_default_language(locale("en"))
            .expect("a default language"),
    )
}

/// `context` served at `/why` in `language`: its bare address in English, `/nl/waarom` in Dutch.
fn addressed(context: HttpContext<()>, language: &str) -> HttpContext<()> {
    let prefix = if language == "en" { "" } else { language };
    let address =
        fundamental_core::address::RequestAddress::new(space(), locale(language), prefix, "/why");
    context
        .with_locale(Some(locale(language)))
        .with_address(Some(address))
}

/// Where the recorder sends `context`, the page left unrendered.
async fn sent_to(context: HttpContext<()>) -> Option<String> {
    let route = Route::html("/why", |_ctx: HttpContext<()>| async move {
        Ok("<h1>Why</h1>".to_owned())
    })
    .middleware(recorder());
    let redirect = context.response_redirect();
    route.dispatch(context, Vec::new()).await.expect("no error");
    redirect
        .take()
        .map(|redirect| redirect.location().to_owned())
}

#[tokio::test]
async fn a_choice_returns_to_the_page_at_its_address_in_the_chosen_language() {
    let on_english = addressed(navigation("/why", "lang=nl&tab=x"), "en");
    assert_eq!(
        sent_to(on_english).await.as_deref(),
        Some("/nl/waarom?tab=x")
    );
    let on_dutch = addressed(navigation("/why", "lang=en"), "nl");
    assert_eq!(sent_to(on_dutch).await.as_deref(), Some("/why"));
}

#[tokio::test]
async fn another_site_s_choice_returns_to_the_page_in_the_language_it_is_in() {
    let context = addressed(cross_site("/why", "lang=en"), "nl");
    assert_eq!(sent_to(context).await.as_deref(), Some("/nl/waarom"));
}

/// The recorder that also answers a reader arriving from elsewhere in their own language.
fn welcoming(policy: &LocalePolicy) -> LocaleChoice {
    LocaleChoice::of(policy, false)
        .expect("the policy reads a cookie")
        .with_arrivals()
}

/// A browser's navigation to `/why` in `language` from another site, `pairs` added or replaced.
fn arriving(language: &str, query: &str, pairs: &[(&str, &str)]) -> HttpContext<()> {
    let mut sent: Vec<_> = [
        ("sec-fetch-mode", "navigate"),
        ("sec-fetch-site", "cross-site"),
    ]
    .into_iter()
    .filter(|(name, _)| !pairs.iter().any(|(given, _)| given == name))
    .collect();
    sent.extend_from_slice(pairs);
    addressed(
        navigation("/why", query).with_headers(headers(&sent)),
        language,
    )
}

/// What `recorder` answers `context` with: its status, where it sends the reader, and its `Vary`.
async fn answered(
    recorder: LocaleChoice,
    context: HttpContext<()>,
) -> (Option<u16>, Option<String>, String) {
    let route = Route::html("/why", |_ctx: HttpContext<()>| async move {
        Ok("<h1>Why</h1>".to_owned())
    })
    .middleware(recorder);
    let status = context.response_status();
    let redirect = context.response_redirect();
    let headers = context.response_headers();
    route.dispatch(context, Vec::new()).await.expect("no error");
    let vary = headers
        .take()
        .into_iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case("vary"))
        .map(|(_, value)| value.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(", ");
    let location = redirect
        .take()
        .map(|redirect| redirect.location().to_owned());
    (status.take(), location, vary)
}

#[tokio::test]
async fn a_reader_arriving_at_the_default_language_is_sent_to_the_page_in_theirs() {
    let context = arriving(
        "en",
        "tab=x",
        &[("accept-language", "nl-BE,nl;q=0.9,en;q=0.5")],
    );
    let (status, location, _) = answered(welcoming(&policy()), context).await;
    assert_eq!(status, Some(302));
    assert_eq!(location.as_deref(), Some("/nl/waarom?tab=x"));
}

#[tokio::test]
async fn a_remembered_choice_beats_the_browser_on_arrival() {
    let dutch = arriving(
        "en",
        "",
        &[("accept-language", "en"), ("cookie", "app-locale=nl")],
    );
    let (_, location, _) = answered(welcoming(&policy()), dutch).await;
    assert_eq!(location.as_deref(), Some("/nl/waarom"));
    let english = arriving(
        "en",
        "",
        &[("accept-language", "nl"), ("cookie", "app-locale=en")],
    );
    assert_eq!(answered(welcoming(&policy()), english).await.1, None);
}

#[tokio::test]
async fn a_link_inside_the_site_is_followed_as_it_is() {
    let context = arriving(
        "en",
        "",
        &[("accept-language", "nl"), ("sec-fetch-site", "same-origin")],
    );
    let (status, location, _) = answered(welcoming(&policy()), context).await;
    assert_eq!((status, location), (None, None));
}

#[tokio::test]
async fn an_address_in_another_language_is_read_in_it() {
    let context = arriving(
        "nl",
        "",
        &[("accept-language", "en"), ("cookie", "app-locale=en")],
    );
    assert_eq!(answered(welcoming(&policy()), context).await.1, None);
}

#[tokio::test]
async fn a_default_language_page_says_what_its_answer_varies_on() {
    let (_, _, vary) = answered(welcoming(&policy()), arriving("en", "", &[])).await;
    for name in [
        "accept-language",
        "cookie",
        "sec-fetch-site",
        "sec-fetch-mode",
        "accept",
    ] {
        assert!(
            vary.split(", ").any(|named| named == name),
            "{name}: {vary}"
        );
    }
}

#[tokio::test]
async fn an_arrival_is_answered_only_where_the_surface_asks_for_it() {
    let context = arriving("en", "", &[("accept-language", "nl")]);
    assert_eq!(answered(recorder(), context).await.1, None);
}

#[tokio::test]
async fn an_islands_fetch_is_never_sent_elsewhere() {
    let context = addressed(
        navigation("/why", "").with_headers(headers(&[
            ("sec-fetch-mode", "cors"),
            ("accept-language", "nl"),
        ])),
        "en",
    );
    let (_, location, vary) = answered(welcoming(&policy()), context).await;
    assert_eq!(location, None);
    // A navigation to this address could have been sent elsewhere, so the answer may not be stored.
    for name in [
        "accept-language",
        "cookie",
        "sec-fetch-site",
        "sec-fetch-mode",
        "accept",
    ] {
        assert!(
            vary.split(", ").any(|named| named == name),
            "{name}: {vary}"
        );
    }
}

#[tokio::test]
async fn a_language_the_page_has_no_address_in_is_not_arrived_at() {
    let german = LocalePolicy::new(locale("en"))
        .with_supported(vec![locale("en"), locale("nl"), locale("de")])
        .with_source(LocaleSource::cookie_then_accept_language("app-locale"));
    let context = arriving("en", "", &[("accept-language", "de")]);
    assert_eq!(answered(welcoming(&german), context).await.1, None);
}

/// A site serving each language at an origin of its own, `context` on `language`'s.
fn on_language_origin(context: HttpContext<()>, language: &str) -> HttpContext<()> {
    use fundamental_core::address::{AddressSpace, RequestAddress, WebOrigin};
    let space = AddressSpace::default()
        .with_language_origins(
            [
                (
                    locale("nl"),
                    WebOrigin::parse("https://shop.example.nl").expect("an origin"),
                ),
                (
                    locale("en"),
                    WebOrigin::parse("https://shop.example.com").expect("an origin"),
                ),
            ],
            &locale("en"),
        )
        .expect("origins");
    let address = RequestAddress::new(std::sync::Arc::new(space), locale(language), "", "/why");
    context
        .with_locale(Some(locale(language)))
        .with_address(Some(address))
}

#[tokio::test]
async fn a_choice_crosses_to_the_origin_its_language_is_served_at() {
    let on_dutch = on_language_origin(navigation("/why", "lang=en&tab=x"), "nl");
    assert_eq!(
        sent_to(on_dutch).await.as_deref(),
        Some("https://shop.example.com/why?tab=x")
    );
    let declined = on_language_origin(cross_site("/why", "lang=en"), "nl");
    assert_eq!(sent_to(declined).await.as_deref(), Some("/why"));
}
