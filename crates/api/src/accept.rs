use std::future::Future;
use std::io;

use fundamental_server::Peer;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

/// A listener the accept loop serves: each connection it accepts, and who it came from.
pub(crate) trait Accept: Send + 'static {
    type Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static;

    fn accept(&self) -> impl Future<Output = io::Result<(Self::Stream, Peer)>> + Send;
}

impl Accept for TcpListener {
    type Stream = TcpStream;

    async fn accept(&self) -> io::Result<(TcpStream, Peer)> {
        let (stream, address) = Self::accept(self).await?;
        Ok((stream, Peer::Network(address)))
    }
}

/// A passed socket's connections come from the processes its file mode admits, not an address.
#[cfg(unix)]
impl Accept for UnixListener {
    type Stream = UnixStream;

    async fn accept(&self) -> io::Result<(UnixStream, Peer)> {
        let (stream, _unnamed) = Self::accept(self).await?;
        Ok((stream, Peer::Activated))
    }
}
