/// Keeps the process's telemetry exporters alive, and flushes them on drop.
///
/// ```rust,no_run
/// # extern crate fundamental_observability;
/// # extern crate fundamental_config;
/// # use fundamental_config::AppSettings;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let app = AppSettings::load("layer0")?;
/// let _telemetry = fundamental_observability::init(&app)?;
/// // … run the service; the guard flushes when `main` returns.
/// # Ok(())
/// # }
/// ```
#[must_use = "dropping the guard flushes and stops telemetry — bind it for the lifetime of the process"]
#[derive(Debug, Default)]
pub struct ObservabilityGuard {
    #[cfg(feature = "otlp")]
    pub(crate) providers: Option<crate::otlp::Providers>,
}

impl Drop for ObservabilityGuard {
    fn drop(&mut self) {
        #[cfg(feature = "otlp")]
        if let Some(providers) = self.providers.take() {
            providers.shutdown();
        }
    }
}
