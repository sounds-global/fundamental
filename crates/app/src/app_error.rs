//! [`AppError`] — what an app refuses at start-up.

use fundamental_macros::Error;

/// What an [`App`](crate::App) refuses to build or serve.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AppError {
    /// The app's language policy reads no cookie, so a reader's choice of language is forgotten.
    #[error(
        code = "SERVE_001",
        message = "the language policy reads no cookie, so a reader's choice would not be remembered"
    )]
    ChoiceUnremembered,

    /// The app declares no state for its pages to be served over (`App::state`).
    #[error(
        code = "SERVE_002",
        message = "the app declares no state to serve its pages over"
    )]
    Stateless,

    /// The manifest declares no `[apps.<app>]` for the app.
    #[error(
        code = "SERVE_003",
        message = "the manifest declares no [apps.{app}] for this app; name the section after its crate, or run it as an instance with FUNDAMENTAL_INSTANCE"
    )]
    Undeclared {
        /// The app's key, as its crate names it.
        app: String,
    },

    /// The process was told to run a surface (`FUNDAMENTAL_SURFACES`) the app does not run.
    #[error(
        code = "SERVE_004",
        message = "the process was told to run the {surface} surface, which the app {app} does not run; declare it in app() and in [apps.{app}.{surface}], or leave it out of FUNDAMENTAL_SURFACES"
    )]
    SurfaceNotRun {
        /// The app's key.
        app: String,
        /// The surface named.
        surface: String,
    },

    /// The app syncs over a store (`App::sync_store`) its manifest declares no database for.
    #[error(
        code = "SERVE_005",
        message = "the app {app} syncs over {store}, but its manifest declares no [apps.{app}.database.{store}]"
    )]
    SyncStoreUndeclared {
        /// The app's key.
        app: String,
        /// The store named, as the manifest spells it.
        store: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    #[test]
    fn each_refusal_has_its_own_code() {
        assert_eq!(AppError::ChoiceUnremembered.error_code(), "SERVE_001");
        assert_eq!(AppError::Stateless.error_code(), "SERVE_002");
        assert_eq!(AppError::Stateless.http_status_code(), 500);
        let undeclared = AppError::Undeclared {
            app: "www".to_owned(),
        };
        assert_eq!(undeclared.error_code(), "SERVE_003");
        let not_run = AppError::SurfaceNotRun {
            app: "www".to_owned(),
            surface: "worker".to_owned(),
        };
        assert_eq!(not_run.error_code(), "SERVE_004");
        let store = AppError::SyncStoreUndeclared {
            app: "www".to_owned(),
            store: "cql",
        };
        assert_eq!(store.error_code(), "SERVE_005");
    }
}
