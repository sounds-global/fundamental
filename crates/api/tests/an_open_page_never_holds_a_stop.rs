//! A page left open on a live-reloading router never holds its server's stop.

use std::time::{Duration, Instant};

use fundamental_api::{ApiServer, Router, ServerConfig, ServerListener};
use fundamental_server::StaticSource;

#[tokio::test]
async fn a_stop_ends_an_open_pages_live_reload_stream() {
    let dist = tempfile::tempdir().expect("a dist");
    std::fs::write(dist.path().join("app.css"), "a{}").expect("a stylesheet");
    let router = Router::<()>::new()
        .static_dir_from("/_fundamental", dist.path(), StaticSource::Disk)
        .expect("mounts the bundle")
        .live_reload()
        .expect("mounts the stream");
    let config = ServerConfig::new("127.0.0.1:0".parse().expect("addr"));
    let listener = ApiServer::new(config, (), router)
        .serve()
        .await
        .expect("serves");
    let mut page = reqwest::Client::new()
        .get(format!(
            "http://{}/_fundamental/live",
            listener.local_addr()
        ))
        .send()
        .await
        .expect("the page connects");
    let hello = page.chunk().await.expect("an event").expect("open");
    assert!(hello.starts_with(b"event: hello"), "{hello:?}");

    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(5), listener.stop())
        .await
        .expect("the stop does not wait for the open page")
        .expect("stops");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
}
