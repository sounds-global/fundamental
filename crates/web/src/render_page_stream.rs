use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Instant,
};

use bytes::Bytes;
use fundamental_core::{
    AnyError, escape,
    i18n::{self, Translator},
};
use fundamental_server::{HttpContext, INTERNAL_CSP_NONCE_HEADER, StreamedResponse};
use futures::{Stream, StreamExt, stream};
use hydration_context::{SharedContext, SsrSharedContext};
use leptos::{nonce::Nonce, prelude::*};
use tracing::debug;

use crate::{
    PageContext, RenderedPage,
    in_language::translator_for,
    page::PageRefusal,
    render_options::RenderOptions,
    render_page::{entity_address, name_the_address},
};

const BODY_MARKER: &str = "<!--fundamental-async-body-->";

/// Builds a complete document as a response stream using Leptos's out-of-order SSR mode.
///
/// It waits for the page's blocking resources and takes the first chunk before sending.
///
/// # Errors
/// The page's answer when a blocking query failed, as a loader's [`PageRefusal`] is answered:
/// the gate for a `401`, the not-found page for any other client status, the failure itself
/// (the error page) for a server status.
pub(crate) async fn render_page_stream_with_options<S, P, F, V, D>(
    ctx: &HttpContext<S>,
    options: RenderOptions,
    provide: P,
    view_fn: F,
    document: D,
) -> Result<StreamedResponse, AnyError>
where
    S: Clone + Send + Sync + 'static,
    P: FnOnce(),
    F: FnOnce() -> V,
    V: RenderHtml + 'static,
    D: Fn(&PageContext, &str) -> String + Copy,
{
    let _ = any_spawner::Executor::init_tokio();
    let page = page_context(ctx);
    let state = (!options.without_state).then(|| ctx.state().clone());
    let title = options.title;
    let shared = Arc::new(SsrSharedContext::new_islands());
    let shared_context: Arc<dyn SharedContext + Send + Sync> = shared.clone();
    let owner = Owner::new_root(Some(shared_context));
    let nonce = Nonce::new();
    let nonce_value = nonce.to_string();
    let render_page = page.clone();
    let translator = translator_for(ctx.locale());
    let html_stream = owner.with(|| {
        if let Some(title) = title {
            render_page.set_title(title.to_string());
        }
        render_page.provide();
        provide_context(nonce);
        if let Some(dispatcher) = ctx.action_dispatcher() {
            provide_context(dispatcher);
        }
        if let Some(state) = state {
            provide_context(state);
        }
        provide();
        let render = || view_fn().to_html_stream_out_of_order();
        match &translator {
            Some(translator) => i18n::with(Arc::clone(translator), render),
            None => render(),
        }
    });

    let mut html_stream = LocaleStream {
        translator,
        stream: Box::pin(html_stream),
    };
    wait_for_blocking(&shared).await;
    if let Some(failure) = page.take_refusal() {
        let route = ctx.route_template().unwrap_or("-");
        return Err(PageRefusal::of_blocking_query(failure).into_answer(route));
    }
    // While the shell is unsent, the first chunk holds every ready blocking resource's content.
    let first_chunk = html_stream.next().await.unwrap_or_default();

    // Status and address are settled before the head, so the canonical URL is in it.
    ctx.set_response_header(INTERNAL_CSP_NONCE_HEADER, nonce_value.clone());
    answer_what_the_page_set(ctx, &page);

    let (prefix, suffix) = document_halves(&page, document);

    let body = DocumentBody {
        prefix,
        first_chunk,
        suffix,
        nonce: nonce_value,
    };
    Ok(body.stream(html_stream, &shared, owner))
}

struct DocumentBody {
    prefix: String,
    first_chunk: String,
    suffix: String,
    nonce: String,
}

impl DocumentBody {
    /// The response: the prefix, the first chunk, the rest, hydration data and the suffix.
    fn stream<S>(self, html_stream: S, shared: &SsrSharedContext, owner: Owner) -> StreamedResponse
    where
        S: Stream<Item = String> + Send + 'static,
    {
        let Self {
            prefix,
            first_chunk,
            suffix,
            nonce,
        } = self;
        let html_stream = stream::once(async move { first_chunk })
            .chain(html_stream)
            .map(|chunk| Ok(Bytes::from(chunk)));
        let hydration_data: Pin<Box<dyn Stream<Item = String> + Send>> = shared
            .pending_data()
            .unwrap_or_else(|| Box::pin(stream::empty()));
        let nonce_attribute = escape(&nonce);
        let hydration = hydration_data.map(move |chunk| {
            Ok(Bytes::from(format!(
                "<script nonce=\"{nonce_attribute}\">{chunk}</script>"
            )))
        });
        let keep_owner = stream::once(async move {
            drop(owner);
            Ok(Bytes::new())
        });
        let body = stream::once(async move { Ok(Bytes::from(prefix)) })
            .chain(html_stream)
            .chain(hydration)
            .chain(stream::once(async move { Ok(Bytes::from(suffix)) }))
            .chain(keep_owner);
        StreamedResponse::with_unknown_length("text/html; charset=utf-8", body)
    }
}

/// Waits until every blocking resource the page created has resolved, nested ones included.
async fn wait_for_blocking(shared: &SsrSharedContext) {
    let started = Instant::now();
    let mut waited = false;
    while let Some(wait) = shared.await_deferred() {
        waited = true;
        wait.await;
    }
    if waited {
        debug!(
            elapsed_ms = started.elapsed().as_millis(),
            "a streamed page waited for its blocking resources"
        );
    }
}

fn document_halves<D>(page: &PageContext, document: D) -> (String, String)
where
    D: Fn(&PageContext, &str) -> String + Copy,
{
    // Generated here, never from request data.
    let shell = RenderedPage::new(page.clone(), BODY_MARKER.to_owned()).into_document(document);
    let (prefix, suffix) = shell
        .split_once(BODY_MARKER)
        .expect("the document shell preserves the body argument");
    (prefix.to_owned(), suffix.to_owned())
}

fn page_context<S>(ctx: &HttpContext<S>) -> PageContext
where
    S: Clone + Send + Sync + 'static,
{
    let params = ctx
        .params()
        .iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    PageContext::new(params, ctx.request_id())
        .with_query(ctx.query_raw())
        .with_principal(ctx.principal_opt().cloned())
        .with_locale(ctx.locale().cloned())
        .with_preferences(ctx.preferences().to_vec())
        .with_address(entity_address(ctx))
        .with_asset_addresses(Arc::clone(ctx.asset_addresses()))
        .with_live_reload(ctx.serves_live_reload())
        .with_document(ctx.extension::<Arc<crate::Document>>().cloned())
        .with_shown_error(ctx.shown_error())
}

/// Puts the status and headers the page set on the response, naming its address on success.
fn answer_what_the_page_set<S>(ctx: &HttpContext<S>, page: &PageContext)
where
    S: Clone + Send + Sync + 'static,
{
    let status = page.take_response_status();
    if let Some(status) = status {
        ctx.set_response_status(status);
    }
    for (name, value) in page.take_response_headers() {
        ctx.set_response_header(name, value);
    }
    let undisclosed =
        ctx.segments().is_declared() && !ctx.segments().disclosable(ctx.authorized_objects());
    if status.is_none_or(|status| (200..300).contains(&status)) && !undisclosed {
        name_the_address(page);
    }
}

/// Renders a page with Leptos's asynchronous SSR executor and streams the document.
///
/// # Errors
/// The page's answer when a blocking query failed: `401` is the gate, any other client status
/// the not-found page, a server status the error page, as a loader's refusal is answered.
pub async fn render_page_stream<S, F, V, D>(
    ctx: &HttpContext<S>,
    view_fn: F,
    document: D,
) -> Result<StreamedResponse, AnyError>
where
    S: Clone + Send + Sync + 'static,
    F: FnOnce() -> V,
    V: RenderHtml + 'static,
    D: Fn(&PageContext, &str) -> String + Copy,
{
    render_page_stream_with_options(ctx, RenderOptions::default(), || {}, view_fn, document).await
}

/// Renders a page with request-loaded data in context and streams the document.
///
/// # Errors
/// As [`render_page_stream`]: the page's answer when a blocking query failed.
pub async fn render_page_stream_with<S, T, F, V, D>(
    ctx: &HttpContext<S>,
    data: T,
    view_fn: F,
    document: D,
) -> Result<StreamedResponse, AnyError>
where
    S: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    F: FnOnce() -> V,
    V: RenderHtml + 'static,
    D: Fn(&PageContext, &str) -> String + Copy,
{
    render_page_stream_with_options(
        ctx,
        RenderOptions::default(),
        move || provide_context(data),
        view_fn,
        document,
    )
    .await
}

struct LocaleStream<S> {
    translator: Option<Arc<dyn Translator>>,
    stream: Pin<Box<S>>,
}

impl<S: Stream<Item = String>> Stream for LocaleStream<S> {
    type Item = String;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        match &this.translator {
            Some(translator) => i18n::with(Arc::clone(translator), || {
                this.stream.as_mut().poll_next(cx)
            }),
            None => this.stream.as_mut().poll_next(cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_server::HttpContext;

    fn document(_: &PageContext, body: &str) -> String {
        format!("<!doctype html><html><body>{body}</body></html>")
    }

    use std::sync::Mutex;
    use tokio::sync::oneshot;

    #[island]
    fn ResourceIsland() -> impl IntoView {
        let receiver = use_context::<Arc<Mutex<Option<oneshot::Receiver<String>>>>>()
            .expect("the test resource gate is in context");
        let message = Resource::new(
            || (),
            move |_| {
                let receiver = Arc::clone(&receiver);
                async move {
                    let receiver = receiver
                        .lock()
                        .expect("not poisoned")
                        .take()
                        .expect("one resource request");
                    receiver.await.expect("the test releases its resource")
                }
            },
        );
        view! {
            <Suspense fallback=|| view! { <p>"Pending resource"</p> }>
                {move || Suspend::new(async move {
                    let value = message.await;
                    view! { <p>{value}</p> }
                })}
            </Suspense>
        }
    }

    #[component]
    fn BlockingTitle() -> impl IntoView {
        let name = Resource::new_blocking(
            || (),
            |_| async {
                tokio::task::yield_now().await;
                "Held project".to_owned()
            },
        );
        view! {
            <Suspense fallback=|| view! { <p>"Pending project"</p> }>
                {move || Suspend::new(async move {
                    let name = name.await;
                    let page = expect_context::<PageContext>();
                    page.set_title(name.clone());
                    page.set_response_status(203);
                    view! { <h1>{name}</h1> }
                })}
            </Suspense>
        }
    }

    fn titled(page: &PageContext, body: &str) -> String {
        let title = page.head().title.unwrap_or_default();
        format!(
            "<!doctype html><html><head><title>{title}</title></head><body>{body}</body></html>"
        )
    }

    #[tokio::test]
    async fn a_blocking_resource_settles_the_head_and_status_before_they_are_sent() {
        let ctx = HttpContext::new((), "stream-test");
        let response = render_page_stream_with_options(
            &ctx,
            RenderOptions::default(),
            || {},
            || view! { <main><BlockingTitle /></main> },
            titled,
        )
        .await
        .expect("the page renders");
        let html = response
            .into_stream()
            .map(|chunk| String::from_utf8(chunk.expect("chunk").to_vec()).expect("UTF-8"))
            .collect::<String>()
            .await;
        assert!(html.contains("<title>Held project</title>"), "{html}");
        assert!(html.contains("<h1>Held project</h1>"), "{html}");
        assert!(!html.contains("Pending project"), "{html}");
        assert_eq!(ctx.response_status().get(), Some(203));
    }

    async fn read_until(
        stream: &mut (impl Stream<Item = Result<Bytes, AnyError>> + Unpin),
        needle: &str,
        chunks: &mut Vec<Bytes>,
    ) -> String {
        let mut read = String::new();
        while !read.contains(needle) {
            let chunk = stream
                .next()
                .await
                .expect("a chunk before the needle")
                .expect("SSR stream chunk");
            read.push_str(std::str::from_utf8(&chunk).expect("UTF-8"));
            chunks.push(chunk);
        }
        read
    }

    #[tokio::test]
    async fn a_streamed_page_links_the_routers_assets_by_version() {
        use fundamental_core::asset::{AssetAddresses, AssetVersion};

        let version =
            AssetVersion::parse("q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA").expect("valid");
        let mut addresses = AssetAddresses::default();
        addresses.insert("/_f/app.css", version.clone());
        let ctx = HttpContext::new((), "stream-test").with_asset_addresses(Arc::new(addresses));
        let response = render_page_stream_with_options(
            &ctx,
            RenderOptions::default(),
            || {},
            || view! { <main>"page"</main> },
            |page: &PageContext, body: &str| {
                let css = page.asset_url("/_f/app.css");
                format!("<!doctype html><html><head><link href=\"{css}\"></head><body>{body}</body></html>")
            },
        )
        .await
        .expect("the page renders");
        let html = response
            .into_stream()
            .map(|chunk| String::from_utf8(chunk.expect("chunk").to_vec()).expect("UTF-8"))
            .collect::<String>()
            .await;
        assert!(html.contains(&format!("/_f/app.css?v={version}")), "{html}");
    }

    #[tokio::test]
    async fn streams_suspense_html_and_serialized_resource_data() {
        let ctx = HttpContext::new((), "stream-test");
        let (release_resource, receiver) = oneshot::channel();
        let receiver = Arc::new(Mutex::new(Some(receiver)));
        let response = render_page_stream_with_options(
            &ctx,
            RenderOptions::default(),
            move || provide_context(receiver),
            || view! { <main><ResourceIsland /></main> },
            document,
        )
        .await
        .expect("the page renders");
        assert_eq!(response.exact_len(), None);
        let mut stream = response.into_stream();
        let mut chunks = Vec::new();
        let pending_html = read_until(&mut stream, "Pending resource", &mut chunks).await;
        assert!(
            !pending_html.contains("streamed-resource"),
            "{pending_html}"
        );
        release_resource
            .send("streamed-resource".to_owned())
            .expect("receiver is waiting");
        chunks.extend(
            stream
                .map(|chunk| chunk.expect("SSR stream chunk"))
                .collect::<Vec<_>>()
                .await,
        );
        let html = chunks
            .into_iter()
            .map(|chunk| String::from_utf8(chunk.to_vec()).expect("UTF-8"))
            .collect::<String>();
        assert!(html.contains("streamed-resource"), "{html}");
        assert!(html.contains("__RESOLVED_RESOURCES=["), "{html}");
        assert!(html.contains("\\\"streamed-resource\\\""), "{html}");
        assert!(
            html.contains("nonce=\""),
            "Leptos stream scripts need the CSP nonce: {html}"
        );
        assert!(html.ends_with("</body></html>"), "{html}");
        assert!(
            ctx.response_headers()
                .take()
                .iter()
                .any(|(name, _)| name == INTERNAL_CSP_NONCE_HEADER)
        );
    }
}
