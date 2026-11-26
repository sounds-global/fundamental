use fundamental_config::AppSettings;
use fundamental_core::AnyError;
use fundamental_runtime::Surface;
use fundamental_server::{Router, ServerConfig};

use crate::Served;

/// What a library adds to an [`App`](crate::App) beside its pages, added with [`App::part`](crate::App::part).
pub trait AppPart<S>: Send + Sync + 'static {
    /// Adds this part's routes and settings to `router` and `server`, once per serve or check.
    ///
    /// # Errors
    /// Whatever this part refuses: its settings, a path another route already serves.
    fn install(
        &self,
        served: &Served,
        router: Router<S>,
        server: ServerConfig,
    ) -> Result<(Router<S>, ServerConfig), AnyError>;

    /// The surfaces this part runs beside the web listener; none by default.
    ///
    /// # Errors
    /// A surface this part cannot build from `served`.
    fn surfaces(&self, served: &Served) -> Result<Vec<Box<dyn Surface>>, AnyError> {
        let _ = served;
        Ok(Vec::new())
    }

    /// What a release check asks of this part beyond building it; nothing by default.
    ///
    /// # Errors
    /// What this part would refuse at start-up.
    fn check(&self, settings: &AppSettings) -> Result<(), AnyError> {
        let _ = settings;
        Ok(())
    }
}
