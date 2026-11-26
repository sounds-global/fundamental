//! A loader's words come out in the reader's language.

use std::sync::Arc;

use fundamental::core::i18n::{self, Arg, Locale, NoopTranslator, Translator, TranslatorSource};
use fundamental::web::{
    LocalePolicy, Router, ServerConfig, ServerListener, WebListener, WebServer,
};
use loader_language_fixture::{AppState, legacy, pipeline};

struct Dutch;

impl Translator for Dutch {
    fn lookup(&self, key: &str, _args: &[Arg<'_>]) -> Option<String> {
        (key == "fixture.word").then(|| "woord".to_owned())
    }
}

struct DutchOnly;

impl TranslatorSource for DutchOnly {
    fn translator(&self, locale: &Locale) -> Arc<dyn Translator> {
        if locale.language() == "nl" {
            Arc::new(Dutch)
        } else {
            Arc::new(NoopTranslator)
        }
    }
}

async fn serve(router: Router<AppState>) -> WebListener {
    i18n::set_translator_source(Arc::new(DutchOnly));
    let english: Locale = "en".parse().expect("a language tag");
    let dutch: Locale = "nl".parse().expect("a language tag");
    let config = ServerConfig::new("127.0.0.1:0".parse().expect("an address"))
        .with_locale_policy(LocalePolicy::new(english).with_supported(vec![dutch]));
    WebServer::new(config, AppState, router)
        .serve()
        .await
        .expect("binds an ephemeral port")
}

async fn word_in_dutch(listener: &WebListener, path: &str) -> String {
    reqwest::Client::new()
        .get(format!("http://{}{path}", listener.local_addr()))
        .header("accept-language", "nl")
        .send()
        .await
        .expect("the request is answered")
        .text()
        .await
        .expect("a text body")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_data_loader_builds_its_words_in_the_readers_language() {
    let listener = serve(legacy::register_pages(Router::new()).expect("registers")).await;
    let html = word_in_dutch(&listener, "/").await;
    assert!(html.contains("<p id=\"word\">woord</p>"), "{html}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_loader_builds_its_words_in_the_readers_language() {
    let listener = serve(pipeline::register_pages(Router::new()).expect("registers")).await;
    let html = word_in_dutch(&listener, "/").await;
    assert!(html.contains("<p id=\"word\">woord</p>"), "{html}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_data_loader_on_the_pipeline_builds_its_words_in_the_readers_language() {
    let listener = serve(pipeline::register_pages(Router::new()).expect("registers")).await;
    let html = word_in_dutch(&listener, "/data").await;
    assert!(html.contains("<p id=\"word\">woord</p>"), "{html}");
}
