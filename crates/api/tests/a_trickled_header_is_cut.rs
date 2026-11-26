//! A connection that never finishes its headers is closed, over a real socket.

use std::time::Duration;

use fundamental_api::{
    AnyError, ApiServer, HttpContext, Route, Router, ServerConfig, ServerListener,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn a_trickled_header_is_cut() {
    let router = Router::new()
        .route(Route::get("/", |_: HttpContext<()>, (): ()| async move {
            Ok::<_, AnyError>(())
        }))
        .expect("unique route");
    let listener = ApiServer::new(
        ServerConfig::new("127.0.0.1:0".parse().expect("addr"))
            .with_header_read_timeout(Duration::from_millis(400)),
        (),
        router,
    )
    .serve()
    .await
    .expect("serves");

    let mut stream = TcpStream::connect(listener.local_addr().address().expect("a bound address"))
        .await
        .expect("connects");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nX-Slow: ")
        .await
        .expect("writes the start");

    let closed = tokio::time::timeout(Duration::from_secs(3), async {
        let mut buffer = [0_u8; 512];
        loop {
            if stream.write_all(b"a").await.is_err() {
                return true;
            }
            match tokio::time::timeout(Duration::from_millis(100), stream.read(&mut buffer)).await {
                Ok(Ok(0)) | Ok(Err(_)) => return true,
                Ok(Ok(_)) => return true,
                Err(_) => {}
            }
        }
    })
    .await
    .unwrap_or(false);

    assert!(
        closed,
        "a connection trickling its headers was still open after 3 s"
    );
    listener.stop().await.expect("stops");
}
