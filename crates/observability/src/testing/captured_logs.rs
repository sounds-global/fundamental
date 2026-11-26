use std::io::{self, Write};
use std::sync::{Arc, Mutex, PoisonError};

use tracing::Subscriber;
use tracing::subscriber::SetGlobalDefaultError;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;

/// Everything a `fmt` layer writes, every level and every span's fields, for a test to read.
#[derive(Clone, Default)]
pub struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

impl CapturedLogs {
    /// Installs a subscriber writing everything here as the process's global default.
    ///
    /// # Errors
    /// [`SetGlobalDefaultError`] if the process already has a global subscriber.
    pub fn install() -> Result<Self, SetGlobalDefaultError> {
        let captured = Self::default();
        let subscriber = tracing_subscriber::registry()
            .with(captured.layer())
            .with(LevelFilter::TRACE);
        tracing::subscriber::set_global_default(subscriber)?;
        Ok(captured)
    }

    /// The formatting layer that writes here, for a test that composes its own subscriber.
    #[must_use]
    pub fn layer<S>(&self) -> impl Layer<S> + use<S>
    where
        S: Subscriber + for<'span> LookupSpan<'span>,
    {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
            .with_writer(self.clone())
    }

    /// Everything written so far.
    #[must_use]
    pub fn text(&self) -> String {
        let bytes = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl Write for CapturedLogs {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for CapturedLogs {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use tracing::{info, info_span};

    use super::*;

    #[test]
    fn a_layer_writes_events_and_span_fields_here() {
        let captured = CapturedLogs::default();
        let subscriber = tracing_subscriber::registry()
            .with(captured.layer())
            .with(LevelFilter::TRACE);
        tracing::subscriber::with_default(subscriber, || {
            let _span = info_span!("boundary", class = "data").entered();
            info!(frame = 3, "refused");
        });
        let text = captured.text();
        assert!(text.contains("refused"), "{text}");
        assert!(text.contains("class=\"data\""), "{text}");
        assert!(text.contains("frame=3"), "{text}");
    }
}
