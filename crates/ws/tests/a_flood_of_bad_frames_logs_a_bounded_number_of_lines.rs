//! A client sending bad frames at wire speed writes a bounded number of lines; the rest are counted.
//!
//! One test in its own binary, because it installs the process's global subscriber.

mod support;

use fundamental_observability::testing::CapturedLogs;
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use support::{config, connect, log_in, router, serve, until};

const FRAMES: usize = 100_000;
/// `ConnectionRefusals`' allowance per window, which one connection cannot outrun.
const LINES_PER_CONNECTION_WINDOW: usize = 10;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn a_flood_of_bad_frames_logs_a_bounded_number_of_lines() {
    let captured = CapturedLogs::install().expect("the only subscriber");
    let (listener, stats) = serve(config(), router()).await;
    let mut socket = connect(&listener).await;
    log_in(&mut socket, "flooder").await;

    let (mut sink, mut source) = socket.split();
    let drain = tokio::spawn(async move {
        let mut replies = 0;
        while replies < FRAMES {
            match source.next().await {
                Some(Ok(Message::Text(_))) => replies += 1,
                Some(Ok(_)) => {}
                other => panic!("the connection ended after {replies} replies: {other:?}"),
            }
        }
        source
    });
    for _ in 0..FRAMES {
        sink.feed(Message::text("not json")).await.expect("send");
    }
    sink.flush().await.expect("flush");
    let source = drain.await.expect("every refusal was answered");
    drop(sink.reunite(source).expect("one socket"));

    until(
        || captured.text().contains("connection closed"),
        "the close line",
    )
    .await;
    let written = captured.text();
    let refusal_lines = written.matches("discarding malformed frame").count();
    assert_eq!(
        refusal_lines, LINES_PER_CONNECTION_WINDOW,
        "{refusal_lines} refusal lines"
    );
    let suppressed = FRAMES - LINES_PER_CONNECTION_WINDOW;
    assert!(
        written.contains(&format!("refusals_suppressed={suppressed}")),
        "the close line does not carry the exact suppressed count"
    );
    assert!(written.contains(&format!("refusals={FRAMES}")));
    assert_eq!(
        stats.refusal_warnings_logged(),
        LINES_PER_CONNECTION_WINDOW as u64
    );
    assert_eq!(stats.refusal_warnings_suppressed(), suppressed as u64);
    // `CapturedLogs` also writes span opens and closes, which a deployment's formatter does not.
    let events = written
        .lines()
        .filter(|line| !line.contains(": new") && !line.contains(": close time."))
        .count();
    assert!(
        events < 50,
        "{events} event lines for one flood:\n{written}"
    );
}
