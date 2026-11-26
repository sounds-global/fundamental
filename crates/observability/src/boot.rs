use fundamental_config::AppSettings;

use crate::{ObservabilityBuilder, ObservabilityError, ObservabilityGuard};

/// Loads `app_name`'s configuration and installs the telemetry it describes.
///
/// The load runs under a bootstrap subscriber (`RUST_LOG`, or `info`, to stdout), so its
/// own diagnostics are kept; the configured subscriber is installed afterwards.
///
/// ```rust,no_run
/// # extern crate fundamental_observability;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let (app, _telemetry) = fundamental_observability::boot("layer0")?;
/// // … run the service; the guard flushes buffered telemetry when `main` returns.
/// # let _ = app;
/// # Ok(())
/// # }
/// ```
///
/// # Errors
/// - [`ObservabilityError::Config`] if the configuration cannot be loaded.
/// - Whatever [`ObservabilityBuilder::init`] reports.
pub fn boot(app_name: &str) -> Result<(AppSettings, ObservabilityGuard), ObservabilityError> {
    let (app, builder) = ObservabilityBuilder::boot(app_name)?;
    let guard = builder.init()?;
    Ok((app, guard))
}

/// Loads `app_name`'s release from `manifest` and checks the telemetry it describes, under the
/// bootstrap subscriber, so a deploy refuses a release that would start blind.
/// ```rust,no_run
/// # extern crate fundamental_observability;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let app = fundamental_observability::check_release("layer0", "next/config/fun.toml")?;
/// // … build every surface the release would run, and start none of them.
/// # let _ = app;
/// # Ok(())
/// # }
/// ```
///
/// # Errors
/// - [`ObservabilityError::Config`] if the release cannot be loaded, or is checked outside a
///   deployed environment (`CFG_030`).
/// - Whatever [`ObservabilityBuilder::check`] reports.
pub fn check_release(
    app_name: &str,
    manifest: impl AsRef<std::path::Path>,
) -> Result<AppSettings, ObservabilityError> {
    use tracing_subscriber::util::SubscriberInitExt;

    // A subscriber already installed is kept.
    let _ = bootstrap_subscriber().try_init();
    let app = AppSettings::load_release(app_name, manifest)?;
    ObservabilityBuilder::from_app(&app).check()?;
    Ok(app)
}

/// Runs `load` with a bootstrap subscriber installed as the thread's default.
pub(crate) fn with_bootstrap_logging<T>(load: impl FnOnce() -> T) -> T {
    tracing::subscriber::with_default(bootstrap_subscriber(), load)
}

/// The bootstrap subscriber: `RUST_LOG`, or `info`, to stdout.
/// Compact `fmt`: the configured format is inside the file being read.
fn bootstrap_subscriber() -> impl tracing::Subscriber + Send + Sync + 'static {
    use tracing_subscriber::layer::SubscriberExt;

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
}

#[cfg(test)]
mod tests {
    use super::with_bootstrap_logging;

    #[test]
    fn a_load_runs_with_a_subscriber_listening() {
        let listening = with_bootstrap_logging(|| tracing::event_enabled!(tracing::Level::INFO));
        assert!(listening, "the load must be observed while it happens");
    }

    #[test]
    fn the_bootstrap_does_not_outlive_the_load() {
        let inside = with_bootstrap_logging(|| tracing::event_enabled!(tracing::Level::INFO));
        let outside = tracing::event_enabled!(tracing::Level::INFO);

        assert!(inside);
        assert!(
            !outside,
            "the bootstrap must be scoped to the load — installed globally it would \
             consume the one slot `tracing` allows, and the *configured* subscriber \
             (the service's format, filters and exporters) would then never install"
        );
    }
}
