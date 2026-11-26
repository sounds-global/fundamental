use std::collections::VecDeque;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};
use std::task::{Context, Poll, Waker};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use fundamental_core::AnyError;
use futures_core::Stream;
use tracing::{debug, info_span, warn};

use crate::bundle_stamps::{BundleChange, BundleStamps};

/// Where the event stream is served.
pub(crate) const LIVE_PATH: &str = "/_fundamental/live";
/// Where the page's client for it is served.
pub(crate) const LIVE_SCRIPT_PATH: &str = "/_fundamental/live.js";
/// The client: swaps stylesheets on `css`, reloads on `reload` or a new server.
pub(crate) const LIVE_SCRIPT: &str = include_str!("live_reload.js");

/// What a router serving the live reload hands every handler.
#[derive(Debug, Clone)]
pub(crate) struct LiveReloadMounted(pub(crate) LiveReload);

/// How often a connection looks at the bundle.
const POLL: Duration = Duration::from_millis(200);
/// How long a change must hold before it is told: `wasm-bindgen` writes its two files in turn.
const SETTLE: Duration = Duration::from_millis(100);
/// How soon a page reconnects when its stream drops, as a restarting server drops it.
const RECONNECT: Duration = Duration::from_millis(250);
/// How long a connection may stay silent before a keep-alive comment.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// The disk-served roots one router watches for its open pages; development only.
#[derive(Debug, Clone)]
pub(crate) struct LiveReload {
    roots: Arc<[PathBuf]>,
    poll: Duration,
    settle: Duration,
    open: Arc<OpenStreams>,
}

/// Every stream a [`LiveReload`] has open, so a stopping server can end them.
#[derive(Debug, Default)]
struct OpenStreams {
    ended: AtomicBool,
    streams: Mutex<Vec<Weak<Shared>>>,
}

impl OpenStreams {
    /// Holds `shared` until it ends, or ends it at once when the server has.
    fn hold(&self, shared: &Arc<Shared>) {
        let mut streams = self.streams.lock().unwrap_or_else(PoisonError::into_inner);
        if self.ended.load(Ordering::Relaxed) {
            shared.end();
            return;
        }
        streams.retain(|held| held.strong_count() > 0);
        streams.push(Arc::downgrade(shared));
    }

    /// Ends every open stream, and every one opened from now on.
    fn end(&self) {
        let streams = self.streams.lock().unwrap_or_else(PoisonError::into_inner);
        self.ended.store(true, Ordering::Relaxed);
        for shared in streams.iter().filter_map(Weak::upgrade) {
            shared.end();
        }
    }
}

impl LiveReload {
    /// Watches `roots` at the development pace.
    pub(crate) fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots: roots.into(),
            poll: POLL,
            settle: SETTLE,
            open: Arc::default(),
        }
    }

    /// Ends every page's stream; the page reconnects and, seeing a new boot id, reloads.
    pub(crate) fn end_open_streams(&self) {
        debug!("ending the open pages' live reload streams");
        self.open.end();
    }

    /// Watches at `poll`, holding each change for `settle` before telling it.
    #[cfg(test)]
    pub(crate) fn paced(mut self, poll: Duration, settle: Duration) -> Self {
        self.poll = poll;
        self.settle = settle;
        self
    }

    /// One page's stream: `hello` with the boot id, then `css` or `reload` per change.
    pub(crate) fn stream(&self) -> LiveStream {
        let shared = Arc::new(Shared::default());
        shared.push(hello());
        self.open.hold(&shared);
        let watcher = Arc::clone(&shared);
        let watch = self.clone();
        let spawned = thread::Builder::new()
            .name("live-reload".to_owned())
            .spawn(move || watch.watch(&watcher));
        if let Err(e) = spawned {
            // The page still learns of a restart by its boot id; only in-place updates are lost.
            warn!(error = %e, "could not watch the bundle; this page will not live-reload");
        }
        LiveStream { shared }
    }

    /// Polls the roots until the page goes, telling it each settled change.
    fn watch(&self, shared: &Shared) {
        let _span = info_span!("live_reload", roots = self.roots.len()).entered();
        let mut seen = BundleStamps::take(&self.roots);
        let mut silent = Duration::ZERO;
        while !shared.is_closed() {
            thread::sleep(self.poll);
            let Some(settled) = self.settled_change(&seen) else {
                silent = keep_alive(shared, silent + self.poll);
                continue;
            };
            if let Some(change) = seen.change(&settled) {
                shared.push(told(change, &settled));
            }
            silent = Duration::ZERO;
            seen = settled;
        }
    }

    /// The bundle once a change from `seen` has held for the settle window, or `None`.
    fn settled_change(&self, seen: &BundleStamps) -> Option<BundleStamps> {
        let next = BundleStamps::take(&self.roots);
        seen.change(&next)?;
        thread::sleep(self.settle);
        let settled = BundleStamps::take(&self.roots);
        (settled == next).then_some(settled)
    }
}

/// After `silent` without an event, a keep-alive comment when due; how long it has been silent since.
fn keep_alive(shared: &Shared, silent: Duration) -> Duration {
    if silent < KEEP_ALIVE {
        return silent;
    }
    shared.push(Bytes::from_static(b": keep-alive\n\n"));
    Duration::ZERO
}

/// The event that tells a page about `change`: `css` or `reload`.
fn told(change: BundleChange, settled: &BundleStamps) -> Bytes {
    debug!(?change, "the bundle changed; telling the page");
    match change {
        BundleChange::Css => event("css", settled.css_stamp().to_string()),
        BundleChange::Reload => event("reload", String::new()),
    }
}

/// This process's boot id, so a page reconnecting to a restarted server reloads.
fn boot_id() -> String {
    static BOOT: OnceLock<String> = OnceLock::new();
    BOOT.get_or_init(|| {
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default();
        format!("{:x}-{started:x}", std::process::id())
    })
    .clone()
}

/// The first event on every stream: the boot id, and a short reconnect delay for `fun dev`.
fn hello() -> Bytes {
    Bytes::from(format!(
        "event: hello\ndata: {}\nretry: {}\n\n",
        boot_id(),
        RECONNECT.as_millis()
    ))
}

/// One server-sent event.
fn event(name: &str, data: String) -> Bytes {
    Bytes::from(format!("event: {name}\ndata: {data}\n\n"))
}

/// What a connection's watcher and its stream share.
#[derive(Default)]
struct Shared {
    events: Mutex<VecDeque<Bytes>>,
    waker: Mutex<Option<Waker>>,
    closed: AtomicBool,
}

impl Shared {
    fn push(&self, bytes: Bytes) {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(bytes);
        self.wake();
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    /// Ends the stream once its queued events are sent, and its watcher with it.
    fn end(&self) {
        self.closed.store(true, Ordering::Relaxed);
        self.wake();
    }

    fn wake(&self) {
        if let Some(waker) = self
            .waker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            waker.wake();
        }
    }
}

/// One open page's events, as a streamed response body; dropping it ends its watcher.
pub(crate) struct LiveStream {
    shared: Arc<Shared>,
}

impl Stream for LiveStream {
    type Item = Result<Bytes, AnyError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // The waker is stored before the queue is read, so no event is missed.
        *self
            .shared
            .waker
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(cx.waker().clone());
        match self
            .shared
            .events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
        {
            Some(bytes) => Poll::Ready(Some(Ok(bytes))),
            None if self.shared.is_closed() => Poll::Ready(None),
            None => Poll::Pending,
        }
    }
}

impl Drop for LiveStream {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    async fn next(stream: &mut LiveStream) -> String {
        let chunk = tokio::time::timeout(
            Duration::from_secs(5),
            std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)),
        )
        .await
        .expect("an event within the deadline")
        .expect("the stream is open")
        .expect("no error");
        String::from_utf8(chunk.to_vec()).expect("text")
    }

    fn watching() -> (tempfile::TempDir, LiveReload) {
        let dir = tempfile::tempdir().expect("a dist");
        fs::write(dir.path().join("app.css"), "a{}").expect("css");
        let live = LiveReload::new(vec![dir.path().to_path_buf()])
            .paced(Duration::from_millis(10), Duration::from_millis(5));
        (dir, live)
    }

    #[tokio::test]
    async fn the_first_event_is_hello_with_the_boot_id_and_a_quick_reconnect() {
        let (_dir, live) = watching();
        let first = next(&mut live.stream()).await;
        assert_eq!(
            first,
            format!("event: hello\ndata: {}\nretry: 250\n\n", boot_id())
        );
    }

    #[tokio::test]
    async fn a_stylesheet_edit_sends_css() {
        let (dir, live) = watching();
        let mut stream = live.stream();
        next(&mut stream).await;
        tokio::time::sleep(Duration::from_millis(30)).await;
        fs::write(dir.path().join("app.css"), "a{color:red}").expect("an edit");
        assert!(next(&mut stream).await.starts_with("event: css\ndata: "));
    }

    #[tokio::test]
    async fn a_wasm_edit_sends_reload() {
        let (dir, live) = watching();
        let mut stream = live.stream();
        next(&mut stream).await;
        tokio::time::sleep(Duration::from_millis(30)).await;
        fs::write(dir.path().join("www_bg.wasm"), b"\0asm").expect("a module");
        assert_eq!(next(&mut stream).await, "event: reload\ndata: \n\n");
    }

    #[test]
    fn a_closed_page_ends_its_watcher() {
        let (_dir, live) = watching();
        let stream = live.stream();
        let shared = Arc::clone(&stream.shared);
        drop(stream);
        assert!(shared.is_closed());
    }
}
