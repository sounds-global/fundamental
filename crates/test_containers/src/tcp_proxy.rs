use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};
use tracing::{debug, warn};

/// Forwards connections on a loopback port to `upstream`, until [`cut`](Self::cut).
///
/// A cut closes every carried and offered connection; [`retarget`](Self::retarget) keeps one
/// address across a container restart.
#[derive(Debug)]
pub struct TcpProxy {
    address: SocketAddr,
    upstream: Arc<Mutex<SocketAddr>>,
    open: Arc<AtomicBool>,
    carried: Arc<Mutex<JoinSet<()>>>,
    accepting: JoinHandle<()>,
}

impl TcpProxy {
    /// Starts forwarding a new loopback port to `upstream`.
    ///
    /// # Errors
    /// The [`std::io::Error`] from binding the port.
    pub async fn start(upstream: SocketAddr) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let upstream = Arc::new(Mutex::new(upstream));
        let open = Arc::new(AtomicBool::new(true));
        let carried = Arc::new(Mutex::new(JoinSet::new()));
        let accepting = tokio::spawn(accept(
            listener,
            Arc::clone(&upstream),
            Arc::clone(&open),
            Arc::clone(&carried),
        ));
        Ok(Self {
            address,
            upstream,
            open,
            carried,
            accepting,
        })
    }

    /// The loopback address clients connect to.
    #[must_use]
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Closes every connection carried and refuses every new one.
    pub fn cut(&self) {
        self.open.store(false, Ordering::SeqCst);
        self.carried
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .abort_all();
        debug!(proxy = %self.address, "proxy cut");
    }

    /// Carries connections again.
    pub fn restore(&self) {
        self.open.store(true, Ordering::SeqCst);
        debug!(proxy = %self.address, "proxy restored");
    }

    /// Forwards every new connection to `upstream`; existing ones keep theirs.
    pub fn retarget(&self, upstream: SocketAddr) {
        *self.upstream.lock().unwrap_or_else(PoisonError::into_inner) = upstream;
        debug!(proxy = %self.address, %upstream, "proxy retargeted");
    }
}

impl Drop for TcpProxy {
    fn drop(&mut self) {
        self.accepting.abort();
        self.carried
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .abort_all();
    }
}

#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn accept(
    listener: TcpListener,
    upstream: Arc<Mutex<SocketAddr>>,
    open: Arc<AtomicBool>,
    carried: Arc<Mutex<JoinSet<()>>>,
) {
    loop {
        let (mut client, _) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                warn!(%error, "proxy accept failed");
                continue;
            }
        };
        if !open.load(Ordering::SeqCst) {
            drop(client);
            continue;
        }
        let upstream = *upstream.lock().unwrap_or_else(PoisonError::into_inner);
        carried
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .spawn(async move {
                match TcpStream::connect(upstream).await {
                    Ok(mut server) => {
                        if let Err(error) =
                            tokio::io::copy_bidirectional(&mut client, &mut server).await
                        {
                            debug!(%error, %upstream, "proxied connection ended with an error");
                        }
                    }
                    Err(error) => warn!(%error, %upstream, "proxy could not reach its upstream"),
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use super::TcpProxy;

    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn echo() -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buffer = [0_u8; 16];
                    while let Ok(read) = socket.read(&mut buffer).await {
                        if read == 0 || socket.write_all(&buffer[..read]).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        address
    }

    async fn round_trip(proxy: &TcpProxy) -> bool {
        let Ok(mut socket) = TcpStream::connect(proxy.address()).await else {
            return false;
        };
        let mut answer = [0_u8; 4];
        socket.write_all(b"ping").await.is_ok()
            && socket.read_exact(&mut answer).await.is_ok()
            && &answer == b"ping"
    }

    #[tokio::test]
    async fn a_cut_proxy_carries_nothing_until_restored() {
        let proxy = TcpProxy::start(echo().await).await.expect("proxy");
        assert!(round_trip(&proxy).await, "an open proxy carries");
        proxy.cut();
        assert!(!round_trip(&proxy).await, "a cut proxy carries nothing");
        proxy.restore();
        assert!(round_trip(&proxy).await, "a restored proxy carries again");
    }

    #[tokio::test]
    async fn a_retargeted_proxy_carries_to_its_new_upstream() {
        let closed = {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            listener.local_addr().expect("address")
        };
        let proxy = TcpProxy::start(closed).await.expect("proxy");
        assert!(!round_trip(&proxy).await, "nothing listens upstream");
        proxy.retarget(echo().await);
        assert!(round_trip(&proxy).await, "the new upstream answers");
    }
}
