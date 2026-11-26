use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing::warn;

/// The exporters installed for this process, shut down by the guard's `Drop`.
#[derive(Debug)]
pub(crate) struct Providers {
    tracer: Option<SdkTracerProvider>,
    logger: Option<SdkLoggerProvider>,
}

impl Providers {
    pub(crate) const fn has_tracer(&self) -> bool {
        self.tracer.is_some()
    }

    pub(crate) const fn has_logger(&self) -> bool {
        self.logger.is_some()
    }

    pub(crate) const fn new(
        tracer: Option<SdkTracerProvider>,
        logger: Option<SdkLoggerProvider>,
    ) -> Self {
        Self { tracer, logger }
    }

    /// Flushes and stops every pipeline, logging a failure: no caller is left to handle it.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) fn shutdown(self) {
        if let Some(tracer) = self.tracer
            && let Err(error) = tracer.shutdown()
        {
            warn!(%error, "OTLP trace pipeline did not flush cleanly; the final spans may be lost");
        }
        if let Some(logger) = self.logger
            && let Err(error) = logger.shutdown()
        {
            warn!(%error, "OTLP log pipeline did not flush cleanly; the final records may be lost");
        }
    }
}
