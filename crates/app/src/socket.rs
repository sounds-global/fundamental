use fundamental_config::AppSettings;
use fundamental_core::AnyError;
use fundamental_ws::{WsConfig, WsRouter, WsServer};

/// The surface name, as `[apps.<name>.socket]` declares it.
pub(crate) const SOCKET: &str = "socket";

/// The step that adds the app's socket routes.
type Routes = Box<dyn Fn(WsRouter) -> WsRouter + Send + Sync>;
/// The step that tunes the server's settings.
type Tune = Box<dyn Fn(WsConfig) -> WsConfig + Send + Sync>;

/// What an app's [`socket`](crate::App::socket) declares.
#[derive(Default)]
pub(crate) struct Socket {
    routes: Option<Routes>,
    tune: Option<Tune>,
}

impl Socket {
    /// Whether the app declares a socket.
    pub(crate) fn declared(&self) -> bool {
        self.routes.is_some()
    }

    /// Declares the routes `register` adds.
    pub(crate) fn routes(&mut self, register: Routes) {
        self.routes = Some(register);
    }

    /// Tunes the server's settings with `tune`.
    pub(crate) fn tune(&mut self, tune: Tune) {
        self.tune = Some(tune);
    }

    /// The server, listening where `settings` say; binds nothing until started.
    pub(crate) fn server(&self, settings: &AppSettings) -> Result<WsServer, AnyError> {
        let config = WsConfig::new(settings.socket_addr()?);
        let config = match &self.tune {
            Some(tune) => tune(config),
            None => config,
        };
        let router = match &self.routes {
            Some(routes) => routes(WsRouter::new()),
            None => WsRouter::new(),
        };
        Ok(WsServer::new(config, router))
    }
}
