use core::time::Duration;
use std::net::Ipv4Addr;

use std::io::{Error as IoError, ErrorKind};
use subtle::ConstantTimeEq as _;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Instant, timeout};
use tracing::{Span, field::Empty, info, instrument, warn};
use uuid::Uuid;

use crate::{CallbackParameters, NativeError};

/// A one-shot HTTP listener on this machine's own loopback, for the redirect leg of
/// an RFC 8252 native-app authorization flow.
///
/// The answer reaches a socket on this machine, which a remote attacker cannot receive on.
///
///
/// ```no_run
/// use core::time::Duration;
/// use fundamental_native::LoopbackCallback;
///
/// # async fn ceremony(finished_page: String) -> Result<(), fundamental_native::NativeError> {
/// // Bound first — the browser cannot be opened before the port exists.
/// let callback = LoopbackCallback::bind(finished_page).await?;
/// let authorize_at = format!("https://example.test/enrol?redirect_uri={}", callback.redirect_uri());
/// // open_in_system_browser(&authorize_at);
/// let delivered = callback.wait(Duration::from_secs(300)).await?;
/// if let Some(code) = delivered.get("code") {
///     // redeem `code`
/// }
/// # Ok(())
/// # }
/// ```
///
/// The socket exists before the browser, the path is a random secret, the redirect is answered
/// once, and everything the peer controls is bounded. The finished page is the caller's, so it
/// is always in the ceremony's language.
#[must_use = "a bound callback that is never waited on holds a port and delivers nothing"]
pub struct LoopbackCallback {
    listener: TcpListener,
    redirect_uri: String,
    path: String,
    finished_page: String,
}

impl LoopbackCallback {
    /// The most a single request's head may weigh before the connection is dropped.
    const MAX_HEAD_BYTES: usize = 8 * 1024;

    /// How many connections are answered before the callback gives up on a local flood.
    const MAX_CONNECTIONS: u32 = 64;

    /// The most parameters a redirect may carry.
    const MAX_PARAMETERS: usize = 16;

    /// Bind an ephemeral port on the IPv4 loopback and draw the path it answers on.
    ///
    /// Call it before opening the browser. `finished_page` is the HTML the browser is left
    /// showing, in the ceremony's language.
    ///
    /// # Errors
    /// [`NativeError::LoopbackBind`] if no loopback port could be bound, or the bound
    /// socket cannot say which one it got.
    #[instrument(name = "native.loopback_bind", skip_all, fields(port = Empty), err(Debug))]
    pub async fn bind(finished_page: impl Into<String>) -> Result<Self, NativeError> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|source| NativeError::LoopbackBind { source })?;
        let port = listener
            .local_addr()
            .map_err(|source| NativeError::LoopbackBind { source })?
            .port();
        // 122 random bits, hyphenated so the path is one URL-safe segment.
        let path = format!("/{}", Uuid::new_v4());
        Span::current().record("port", port);
        info!(port, "loopback callback listening");
        Ok(Self {
            redirect_uri: format!("http://{}:{port}{path}", Ipv4Addr::LOCALHOST),
            listener,
            path,
            finished_page: finished_page.into(),
        })
    }

    /// The absolute URL the authorization server must deliver the answer to.
    ///
    /// The IPv4 loopback literal, never `localhost`, as RFC 8252 §8.3 asks.
    #[must_use]
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// The port that was bound.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or_default()
    }

    /// Wait for the redirect, and hand back what it carried.
    ///
    ///
    /// Anything but this callback's path from a loopback `Host` is answered `404` and the loop
    /// continues, so a stray probe costs nothing.
    ///
    /// # Errors
    /// [`NativeError::LoopbackExpired`] if `within` elapses with no redirect, and
    /// [`NativeError::LoopbackFlooded`] if `MAX_CONNECTIONS` connections came without it.
    #[instrument(
        name = "native.loopback_wait",
        skip_all,
        fields(port = self.port(), answered = Empty),
        err(Debug)
    )]
    pub async fn wait(self, within: Duration) -> Result<CallbackParameters, NativeError> {
        let span = Span::current();
        let deadline = Instant::now() + within;
        let mut answered: u32 = 0;

        loop {
            if answered >= Self::MAX_CONNECTIONS {
                span.record("answered", answered);
                warn!(
                    answered,
                    "loopback callback gave up: something local is answering on its port"
                );
                return Err(NativeError::LoopbackFlooded);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                span.record("answered", answered);
                return Err(NativeError::LoopbackExpired);
            }

            let Ok(accepted) = timeout(remaining, self.listener.accept()).await else {
                span.record("answered", answered);
                return Err(NativeError::LoopbackExpired);
            };
            let mut stream = match accepted {
                Ok((stream, _)) => stream,
                Err(error) => {
                    // Never fatal: one failed accept says nothing of the next.
                    warn!(error = %error, "loopback callback could not accept a connection");
                    answered = answered.saturating_add(1);
                    continue;
                }
            };
            answered = answered.saturating_add(1);

            let remaining = deadline.saturating_duration_since(Instant::now());
            match timeout(remaining, self.serve(&mut stream)).await {
                Ok(Some(parameters)) => {
                    span.record("answered", answered);
                    info!(answered, "loopback callback received the redirect");
                    return Ok(parameters);
                }
                // Already answered; the reason is not reported to the sender.
                Ok(None) => {}
                Err(_) => {
                    span.record("answered", answered);
                    return Err(NativeError::LoopbackExpired);
                }
            }
        }
    }

    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    async fn serve(&self, stream: &mut TcpStream) -> Option<CallbackParameters> {
        let head = match read_head(stream, Self::MAX_HEAD_BYTES).await {
            Ok(head) => head,
            Err(error) => {
                warn!(error = %error, "loopback callback could not read a request");
                return None;
            }
        };

        let Some((path, query)) = requested_target(&head) else {
            respond(stream, "400 Bad Request", "").await;
            return None;
        };
        // Constant time, so the path cannot be walked one byte at a time.
        if path.as_bytes().ct_ne(self.path.as_bytes()).into() {
            respond(stream, "404 Not Found", "").await;
            return None;
        }
        // RFC 8252 §8.3: a rebinding page carries its own name in `Host`.
        if !host_is_loopback(&head) {
            warn!("loopback callback refused a request naming a non-loopback host");
            respond(stream, "404 Not Found", "").await;
            return None;
        }

        respond(stream, "200 OK", &self.finished_page).await;
        Some(CallbackParameters::parse(query, Self::MAX_PARAMETERS))
    }
}

/// Reads up to the end of the request head, bounded in bytes.
async fn read_head(stream: &mut TcpStream, limit: usize) -> Result<String, IoError> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    loop {
        if let Some(end) = find_head_end(&buffer) {
            buffer.truncate(end);
            return String::from_utf8(buffer).map_err(|_| {
                IoError::new(
                    ErrorKind::InvalidData,
                    "the request head is not valid UTF-8",
                )
            });
        }
        if buffer.len() >= limit {
            return Err(IoError::new(
                ErrorKind::InvalidData,
                "the request head exceeded its bound",
            ));
        }
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(IoError::new(
                ErrorKind::UnexpectedEof,
                "the connection closed before a request head",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

fn find_head_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

/// The path and query of a `GET` request line, or `None` for anything else, since a redirect
/// is a navigation.
fn requested_target(head: &str) -> Option<(&str, &str)> {
    let line = head.lines().next()?;
    let mut parts = line.split(' ');
    if parts.next()? != "GET" {
        return None;
    }
    let target = parts.next()?;
    parts.next()?;
    Some(match target.split_once('?') {
        Some((path, query)) => (path, query),
        None => (target, ""),
    })
}

fn host_is_loopback(head: &str) -> bool {
    head.lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("host"))
        .map(|(_, value)| {
            let value = value.trim();
            let host = value.strip_prefix('[').map_or_else(
                || value.split(':').next().unwrap_or(value),
                |rest| rest.split(']').next().unwrap_or(rest),
            );
            host == "127.0.0.1" || host == "::1"
        })
        .unwrap_or(false)
}

/// Writes one complete response and closes; failures are logged and swallowed.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    // `no-store` keeps the page out of the cache; `no-referrer` keeps the code out of `Referer`.
    let response = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {length}\r\n\
         Cache-Control: no-store\r\n\
         Referrer-Policy: no-referrer\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        length = body.len()
    );
    if let Err(error) = stream.write_all(response.as_bytes()).await {
        warn!(error = %error, "loopback callback could not write its response");
        return;
    }
    if let Err(error) = stream.shutdown().await {
        warn!(error = %error, "loopback callback could not close a connection");
    }
}

#[cfg(test)]
mod tests {
    use super::{LoopbackCallback, host_is_loopback, requested_target};
    use core::time::Duration;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpStream;

    async fn request(target: &str, host: &str) -> String {
        let mut stream = TcpStream::connect(target_address(target))
            .await
            .expect("the callback is listening");
        let path = target.split_once("|").map_or(target, |(_, path)| path);
        stream
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes())
            .await
            .expect("write the request");
        let mut answer = String::new();
        stream
            .read_to_string(&mut answer)
            .await
            .expect("read the answer");
        answer
    }

    fn target_address(target: &str) -> String {
        target
            .split_once('|')
            .map_or_else(|| target.to_owned(), |(address, _)| address.to_owned())
    }

    #[tokio::test]
    async fn the_redirect_is_delivered_on_its_own_path() {
        let callback = LoopbackCallback::bind("<p>finished</p>")
            .await
            .expect("bind a loopback port");
        let port = callback.port();
        let path = callback
            .redirect_uri()
            .rsplit_once('/')
            .map(|(_, tail)| format!("/{tail}"))
            .expect("the redirect uri carries a path");

        let driver = tokio::spawn(async move {
            request(
                &format!("127.0.0.1:{port}|{path}?code=the-grant"),
                &format!("127.0.0.1:{port}"),
            )
            .await
        });
        let delivered = callback
            .wait(Duration::from_secs(5))
            .await
            .expect("the redirect arrives");
        let answer = driver.await.expect("the driver finishes");

        assert_eq!(
            delivered
                .get("code")
                .and_then(|code| code.expose())
                .map(String::as_str),
            Some("the-grant")
        );
        assert!(answer.contains("200 OK"), "{answer}");
        assert!(answer.contains("<p>finished</p>"), "{answer}");
        assert!(answer.contains("Cache-Control: no-store"), "{answer}");
    }

    /// Knowing the port is not enough to deliver.
    #[tokio::test]
    async fn a_request_on_another_path_is_refused_and_the_ceremony_survives() {
        let callback = LoopbackCallback::bind("<p>finished</p>")
            .await
            .expect("bind a loopback port");
        let port = callback.port();
        let path = callback
            .redirect_uri()
            .rsplit_once('/')
            .map(|(_, tail)| format!("/{tail}"))
            .expect("the redirect uri carries a path");

        let driver = tokio::spawn(async move {
            let guessed = request(
                &format!("127.0.0.1:{port}|/callback?code=planted"),
                &format!("127.0.0.1:{port}"),
            )
            .await;
            let real = request(
                &format!("127.0.0.1:{port}|{path}?code=the-grant"),
                &format!("127.0.0.1:{port}"),
            )
            .await;
            (guessed, real)
        });
        let delivered = callback
            .wait(Duration::from_secs(5))
            .await
            .expect("the real redirect still arrives");
        let (guessed, real) = driver.await.expect("the driver finishes");

        assert!(guessed.contains("404 Not Found"), "{guessed}");
        assert!(real.contains("200 OK"), "{real}");
        assert_eq!(
            delivered
                .get("code")
                .and_then(|code| code.expose())
                .map(String::as_str),
            Some("the-grant"),
            "the planted code was delivered instead of the real one"
        );
    }

    /// RFC 8252 §8.3's rebinding case is refused.
    #[tokio::test]
    async fn a_request_naming_a_non_loopback_host_is_refused() {
        let callback = LoopbackCallback::bind("<p>finished</p>")
            .await
            .expect("bind a loopback port");
        let port = callback.port();
        let path = callback
            .redirect_uri()
            .rsplit_once('/')
            .map(|(_, tail)| format!("/{tail}"))
            .expect("the redirect uri carries a path");

        let driver = tokio::spawn(async move {
            request(
                &format!("127.0.0.1:{port}|{path}?code=rebound"),
                "attacker.example",
            )
            .await
        });
        let expired = callback.wait(Duration::from_millis(600)).await;
        let answer = driver.await.expect("the driver finishes");

        assert!(answer.contains("404 Not Found"), "{answer}");
        assert!(
            expired.is_err(),
            "a rebinding request was accepted as the redirect"
        );
    }

    #[tokio::test]
    async fn nothing_arriving_expires_rather_than_hanging() {
        let callback = LoopbackCallback::bind("<p>finished</p>")
            .await
            .expect("bind a loopback port");
        assert!(callback.wait(Duration::from_millis(150)).await.is_err());
    }

    /// The redirect URI names the literal and carries no query or fragment.
    #[tokio::test]
    async fn the_redirect_uri_is_a_bare_loopback_literal() {
        let callback = LoopbackCallback::bind("<p>finished</p>")
            .await
            .expect("bind a loopback port");
        let uri = callback.redirect_uri();
        assert!(uri.starts_with("http://127.0.0.1:"), "{uri}");
        assert!(!uri.contains("localhost"), "{uri}");
        assert!(!uri.contains('?') && !uri.contains('#'), "{uri}");
    }

    /// Two callbacks never share a path.
    #[tokio::test]
    async fn two_callbacks_answer_on_different_paths() {
        let one = LoopbackCallback::bind("<p>a</p>").await.expect("bind one");
        let two = LoopbackCallback::bind("<p>b</p>").await.expect("bind two");
        assert_ne!(one.redirect_uri(), two.redirect_uri());
    }

    #[test]
    fn only_a_get_request_line_names_a_target() {
        assert_eq!(
            requested_target("GET /path?a=b HTTP/1.1\r\nHost: 127.0.0.1"),
            Some(("/path", "a=b"))
        );
        assert_eq!(
            requested_target("GET /path HTTP/1.1\r\nHost: 127.0.0.1"),
            Some(("/path", ""))
        );
        assert_eq!(requested_target("POST /path HTTP/1.1"), None);
        assert_eq!(requested_target("GET /path"), None);
        assert_eq!(requested_target(""), None);
    }

    #[test]
    fn only_the_two_loopback_literals_are_loopback_hosts() {
        for head in [
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:9000",
            "GET / HTTP/1.1\r\nhost: 127.0.0.1",
            "GET / HTTP/1.1\r\nHost: [::1]:9000",
        ] {
            assert!(host_is_loopback(head), "{head}");
        }
        for head in [
            "GET / HTTP/1.1\r\nHost: attacker.example",
            "GET / HTTP/1.1\r\nHost: 127.0.0.1.attacker.example",
            "GET / HTTP/1.1\r\nHost: localhost:9000",
            "GET / HTTP/1.1",
        ] {
            assert!(!host_is_loopback(head), "{head}");
        }
    }
}
