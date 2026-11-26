//! The Fundamental development loop: watch, rebuild, restart.
//!
//! ```no_run
//! use std::time::Duration;
//! use fundamental_dev::{BuildPlan, DevApp, DevLoop};
//!
//! # fn main() -> Result<(), fundamental_dev::DevError> {
//! let plan = BuildPlan::new(vec![
//!     DevApp::new("layer0"),
//!     DevApp::new("layer5_witness").bin("witness"),
//! ]);
//!
//! DevLoop::new("/path/to/project", "/path/to/project/target", plan)
//!     .watch_roots(vec!["/path/to/project/domain".into()])
//!     .debounce(Duration::from_millis(250))
//!     .run()?;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod app_process;
mod build_plan;
mod dev_app;
mod dev_error;
mod dev_loop;
mod source_scan;
mod stop_outcome;

pub use app_process::AppProcess;
pub use build_plan::BuildPlan;
pub use dev_app::DevApp;
pub use dev_error::DevError;
pub use dev_loop::DevLoop;
pub use source_scan::SourceScan;
pub use stop_outcome::StopOutcome;

/// The result of a development-loop operation.
pub type DevResult<T> = Result<T, DevError>;
