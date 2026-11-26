use std::sync::Once;

use tracing_subscriber::EnvFilter;

static INSTALLED: Once = Once::new();

/// Makes the harness — and `testcontainers` itself — audible, exactly once per process.
///
/// `testcontainers` reports failed removals and ambiguous reuse through `log`; raise the level
/// with `RUST_LOG=fundamental_test_containers=debug,testcontainers=debug`. `try_init`, so a
/// suite's own subscriber wins.
pub(crate) fn install_harness_logger() {
    INSTALLED.call_once(|| {
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("warn,fundamental_test_containers=info"));
        let installed = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_test_writer()
            .try_init();
        if let Err(error) = installed {
            // On stderr, so the note cannot be filtered by the subscriber it reports.
            eprintln!("test containers: keeping the subscriber already installed ({error})");
        }
    });
}
