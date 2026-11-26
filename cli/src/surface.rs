//! The [`Surface`] a generated app can expose.

use clap::ValueEnum;

/// A runtime surface a generated app can expose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum)]
pub(crate) enum Surface {
    /// NATS consumer: subscribes to subjects and processes messages.
    Worker,
    /// WebSocket server: action-routed, typed message handlers.
    Socket,
    /// HTTP API server: typed request/response handlers on a router.
    Api,
    /// Server-rendered web: SSR pages with opt-in `#[island]` interactivity.
    Web,
    /// Slice-sync WebSocket server: serves offline-first `#[slice]` subscriptions.
    Sync,
    /// Native (Tauri) desktop + mobile app: the same pages served in-process.
    Native,
}

impl Surface {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Surface::Worker => "worker",
            Surface::Socket => "socket",
            Surface::Api => "api",
            Surface::Web => "web",
            Surface::Sync => "sync",
            Surface::Native => "native",
        }
    }

    /// The default listen port for this surface when none is given.
    pub(crate) fn default_port(self) -> u16 {
        match self {
            // A native app's pages are served in-process.
            Surface::Worker | Surface::Socket | Surface::Native => 9000,
            Surface::Api => 16001,
            Surface::Web => 11001,
            Surface::Sync => 9100,
        }
    }
}
