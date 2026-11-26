use std::backtrace::Backtrace;
use std::panic::{self, PanicHookInfo};
use std::thread;

use tracing::error;

/// Replaces the process's panic hook with one that logs a panic's location, never its
/// message, which can quote a request's data.
///
/// [`init`](crate::ObservabilityBuilder::init) installs it; a process with its own subscriber
/// calls it once after installing one.
pub fn install_panic_hook() {
    panic::set_hook(Box::new(report));
}

fn report(info: &PanicHookInfo<'_>) {
    let location = info.location();
    error!(
        target: "fundamental::panic",
        file = location.map(|at| at.file()),
        line = location.map(|at| at.line()),
        column = location.map(|at| at.column()),
        thread = thread::current().name().unwrap_or("<unnamed>"),
        backtrace = %Backtrace::capture(),
        "a thread panicked; its message is not logged, because it can quote the data it tripped on"
    );
}
