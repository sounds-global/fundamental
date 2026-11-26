//! [`DevError`], the error returned by the development loop.

use std::path::PathBuf;

use fundamental_core::AnyError;
use fundamental_macros::Error;

/// An error from watching sources, rebuilding, or supervising a dev process.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DevError {
    /// A lower-layer Fundamental error, preserved losslessly.
    Other(AnyError),

    /// A watched root could not be read.
    #[error(
        code = "DEV_001",
        message = "cannot watch '{}': {reason}", root.display(),
        status = 500
    )]
    UnwatchableRoot {
        /// The root that could not be read.
        root: PathBuf,
        /// Why it could not be read.
        reason: String,
    },

    /// The build command could not be launched at all (cargo missing from `PATH`).
    #[error(
        code = "DEV_002",
        message = "cannot launch `{tool}`: {reason}",
        status = 500
    )]
    ToolLaunch {
        /// The tool that could not be started.
        tool: String,
        /// The underlying OS error.
        reason: String,
    },

    /// A supervised app's binary could not be launched.
    #[error(
        code = "DEV_003",
        message = "cannot start `{app}` from '{}': {reason}", binary.display(),
        status = 500
    )]
    AppLaunch {
        /// The app whose process failed to start.
        app: String,
        /// The binary that was to be executed.
        binary: PathBuf,
        /// The underlying OS error.
        reason: String,
    },

    /// The loop was asked to supervise no apps at all.
    #[error(
        code = "DEV_004",
        message = "no apps to run: the dev loop needs at least one",
        status = 400
    )]
    NothingToRun,
}
