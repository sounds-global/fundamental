//! [`accept_loop`]: accepts and admits connections until the server is stopped, then drains.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures::future::OptionFuture;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio::time::{MissedTickBehavior, interval, sleep};
use tracing::{error, info, warn};

use crate::config::WsConfig;
use crate::error::WsResult;
use crate::observability::{PROCESS_WINDOW, millis};
use crate::routing::WsRouter;
use crate::server::abort_on_drop::AbortOnDrop;
use crate::server::drain_sessions::drain_sessions;
use crate::session::ws_session;
use crate::slots::{AdmissionControl, SlotGuard};

/// Pause after a failed accept (usually out of file descriptors), so the loop does not spin.
const ACCEPT_ERROR_PAUSE: Duration = Duration::from_millis(100);

type Admitting = Pin<Box<dyn Future<Output = (TcpStream, WsResult<SlotGuard>)> + Send>>;

/// Accepts and serves connections until `stop` fires, then drains them; every session runs in one owned
/// [`JoinSet`], and a slot wait is a `select!` branch so a full server still sees the stop.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) async fn accept_loop(
    listener: TcpListener,
    config: Arc<WsConfig>,
    router: Arc<WsRouter>,
    control: Arc<AdmissionControl>,
    mut stop: oneshot::Receiver<()>,
) {
    let _reporter = AbortOnDrop::new(tokio::spawn(report_suppressed(Arc::clone(&control))));
    let mut sessions = JoinSet::new();
    let mut admitting: Option<Admitting> = None;
    loop {
        tokio::select! {
            biased;
            _ = &mut stop => break,
            Some(ended) = sessions.join_next() => {
                if let Err(error) = ended {
                    error!(%error, "a connection's session panicked");
                }
            }
            Some((stream, admitted)) = OptionFuture::from(admitting.as_mut()) => {
                admitting = None;
                match admitted {
                    Ok(guard) => {
                        sessions.spawn(ws_session::run(
                            stream,
                            Arc::clone(&router),
                            Arc::clone(&config),
                            guard,
                        ));
                    }
                    Err(error) => {
                        error!(%error, "connection slots unavailable; the server stops accepting");
                        break;
                    }
                }
            }
            accepted = listener.accept(), if admitting.is_none() => match accepted {
                Ok((stream, _peer)) => {
                    let control = Arc::clone(&control);
                    admitting = Some(Box::pin(async move {
                        let admitted = control.admit().await;
                        (stream, admitted)
                    }));
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        pause_ms = millis(ACCEPT_ERROR_PAUSE),
                        "failed to accept connection"
                    );
                    sleep(ACCEPT_ERROR_PAUSE).await;
                }
            },
        }
    }
    // The listener goes first, so the kernel refuses new connections from here.
    drop(listener);
    drop(admitting);
    info!("websocket server stopped accepting");
    drain_sessions(sessions, &control, &config).await;
}

async fn report_suppressed(control: Arc<AdmissionControl>) {
    let mut every_window = interval(PROCESS_WINDOW);
    every_window.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        every_window.tick().await;
        control.process_refusals().flush();
    }
}
