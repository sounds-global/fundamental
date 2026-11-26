use std::future::Future;
use std::pin::pin;
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder;
use hyper_util::server::graceful::GracefulShutdown;
use hyper_util::service::TowerToHyperService;
use tower::ServiceExt;
use tracing::{debug, warn};

use crate::accept::Accept;
use crate::write_stall_guard::WriteStallGuard;

/// Serves `app` on every connection `listener` accepts until `shutdown`, then drains them.
///
/// Not `axum::serve`, which gives hyper no timer and so no header-read timeout.
pub(crate) async fn serve_connections(
    listener: impl Accept,
    app: axum::Router,
    header_read_timeout: Duration,
    write_stall_timeout: Duration,
    shutdown: impl Future<Output = ()>,
) {
    let graceful = GracefulShutdown::new();
    let mut shutdown = pin!(shutdown);
    loop {
        let (stream, peer) = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(accepted) => accepted,
                Err(error) => {
                    // Usually out of file descriptors: back off rather than spin.
                    warn!(%error, "could not accept a connection");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            },
            () = &mut shutdown => break,
        };
        let service = TowerToHyperService::new(app.clone().map_request(
            move |mut request: Request<Incoming>| {
                request.extensions_mut().insert(peer);
                request.map(Body::new)
            },
        ));
        let mut builder = Builder::new(TokioExecutor::new());
        builder
            .http1()
            .timer(TokioTimer::new())
            .header_read_timeout(header_read_timeout);
        builder.http2().timer(TokioTimer::new());
        let connection = builder
            .serve_connection_with_upgrades(
                TokioIo::new(WriteStallGuard::new(stream, write_stall_timeout)),
                service,
            )
            .into_owned();
        let watched = graceful.watch(connection);
        tokio::spawn(async move {
            if let Err(error) = watched.await {
                debug!(%error, "a connection ended with an error");
            }
        });
    }
    graceful.shutdown().await;
}
