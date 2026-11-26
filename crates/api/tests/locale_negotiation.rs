//! The language a request is answered in, and what the response varies on, end to end.

use fundamental_api::{
    AnyError, ApiServer, HttpContext, Route, Router, ServerConfig, ServerListener,
};
use fundamental_core::i18n::Locale;
use fundamental_server::{LocalePolicy, LocaleSource};

fn locale(tag: &str) -> Locale {
    tag.parse().expect("a language tag")
}

fn router() -> Router<()> {
    Router::new()
        .route(Route::html("/", |ctx: HttpContext<()>| async move {
            Ok::<_, AnyError>(
                ctx.locale()
                    .map_or_else(|| "none".to_owned(), Locale::to_string),
            )
        }))
        .expect("unique route")
}

async fn start(config: ServerConfig) -> fundamental_api::ApiListener {
    ApiServer::new(config, (), router())
        .serve()
        .await
        .expect("binds an ephemeral port")
}

fn config() -> ServerConfig {
    ServerConfig::new("127.0.0.1:0".parse().expect("addr"))
}

/// Every header the response declares it varies on, lowercased, across fields and values.
fn vary(response: &reqwest::Response) -> Vec<String> {
    response
        .headers()
        .get_all("vary")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|value| value.trim().to_ascii_lowercase())
        .collect()
}

#[tokio::test]
async fn without_a_policy_nothing_is_negotiated_and_nothing_varies() {
    let listener = start(config()).await;
    let url = format!("http://{}/", listener.local_addr());

    let response = reqwest::Client::new()
        .get(&url)
        .header("accept-language", "nl")
        .send()
        .await
        .expect("request sent");
    assert_eq!(response.status(), 200);
    assert!(
        !vary(&response).contains(&"accept-language".to_owned()),
        "a surface that negotiates nothing must not claim to vary on the header it never read"
    );
    assert_eq!(response.text().await.expect("text body"), "none");

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn a_policy_answers_in_the_language_the_visitor_asked_for() {
    let policy = LocalePolicy::new(locale("en")).with_supported(vec![locale("nl"), locale("en")]);
    let listener = start(config().with_locale_policy(policy)).await;
    let url = format!("http://{}/", listener.local_addr());
    let client = reqwest::Client::new();

    let dutch = client
        .get(&url)
        .header("accept-language", "nl-BE,nl;q=0.9,en;q=0.5")
        .send()
        .await
        .expect("request sent");
    assert!(
        vary(&dutch).contains(&"accept-language".to_owned()),
        "a page in two languages is two representations of one URL"
    );
    assert_eq!(dutch.text().await.expect("text body"), "nl");

    let fallback = client
        .get(&url)
        .header("accept-language", "ja")
        .send()
        .await
        .expect("request sent");
    assert_eq!(
        fallback.text().await.expect("text body"),
        "en",
        "a language nothing is offered in falls back rather than refusing the reader"
    );

    let unstated = client.get(&url).send().await.expect("request sent");
    assert_eq!(unstated.text().await.expect("text body"), "en");

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn a_locale_cookie_beats_the_header_and_the_response_varies_on_both() {
    let policy = LocalePolicy::new(locale("en"))
        .with_supported(vec![locale("nl"), locale("en")])
        .with_source(LocaleSource::cookie_then_accept_language("locale"));
    let listener = start(config().with_locale_policy(policy)).await;
    let url = format!("http://{}/", listener.local_addr());

    let response = reqwest::Client::new()
        .get(&url)
        .header("accept-language", "en")
        .header("cookie", "locale=nl")
        .send()
        .await
        .expect("request sent");
    let declared = vary(&response);
    assert!(
        declared.contains(&"accept-language".to_owned()),
        "{declared:?}"
    );
    assert!(declared.contains(&"cookie".to_owned()), "{declared:?}");
    assert_eq!(
        declared.iter().filter(|value| *value == "cookie").count(),
        1,
        "a value already declared is not repeated: {declared:?}"
    );
    assert_eq!(
        response.text().await.expect("text body"),
        "nl",
        "an explicit choice must beat the browser's configuration"
    );

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn a_market_addressed_page_declares_no_language_vary() {
    use fundamental_core::address::{AddressSpace, LocalePrefixSet, PageSlug, SlugTable};
    use std::sync::Arc;

    let prefixes = LocalePrefixSet::new()
        .with("nl", locale("nl-NL"))
        .expect("nl")
        .with("nl-en", locale("en-NL"))
        .expect("nl-en");
    let slugs = SlugTable::builder()
        .page("/why", &[PageSlug::current("nl", "waarom")])
        .build()
        .expect("well formed");
    let space = Arc::new(AddressSpace::new(prefixes, slugs));

    let router = Router::new()
        .route(Route::html("/why", |ctx: HttpContext<()>| async move {
            Ok::<_, AnyError>(
                ctx.locale()
                    .map_or_else(|| "none".to_owned(), Locale::to_string),
            )
        }))
        .expect("unique route")
        .route(Route::html("/", |ctx: HttpContext<()>| async move {
            Ok::<_, AnyError>(
                ctx.locale()
                    .map_or_else(|| "none".to_owned(), Locale::to_string),
            )
        }))
        .expect("unique route")
        .with_addresses(space);

    let listener = ApiServer::new(
        config().with_locale_policy(
            LocalePolicy::new(locale("en"))
                .with_supported(vec![locale("nl-NL"), locale("en-NL")])
                .with_source(LocaleSource::path()),
        ),
        (),
        router,
    )
    .serve()
    .await
    .expect("binds an ephemeral port");

    let url = format!("http://{}/nl/waarom", listener.local_addr());
    let response = reqwest::Client::new()
        .get(&url)
        .header("accept-language", "de")
        .send()
        .await
        .expect("request sent");

    assert_eq!(response.status(), 200);
    // The CORS layer legitimately declares `Vary: Origin`; only a language vary is wrong.
    let declared = vary(&response);
    assert!(
        !declared.contains(&"accept-language".to_owned()),
        "the URL is already the whole cache key: {declared:?}"
    );
    assert!(
        !declared.contains(&"cookie".to_owned()),
        "no cookie decides this language either: {declared:?}"
    );
    assert_eq!(
        response.text().await.expect("text body"),
        "nl-NL",
        "the address decides the language, not the browser's configuration"
    );

    listener.stop().await.expect("stops cleanly");
}
