use std::time::Duration;

use fundamental_config::settings::ObservabilitySettings;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::trace::{Sampler, SdkTracerProvider};
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::registry::LookupSpan;

use crate::ObservabilityError;
use crate::otlp::{Providers, Transport};

/// The instrumentation scope every span this process emits is attributed to.
const SCOPE: &str = "fundamental";

type ExportLayers<S> = Vec<Box<dyn Layer<S> + Send + Sync>>;

/// The resolved OTLP configuration for one process.
pub(crate) struct OtlpPlan {
    enabled: bool,
    traces: bool,
    endpoint: String,
    protocol: String,
    timeout: Duration,
    sample_ratio: f64,
    always_sample_errors: bool,
}

impl OtlpPlan {
    /// Reads `[observability]`; `tracing_enabled` lets a service ship logs without spans.
    pub(crate) fn from_settings(settings: &ObservabilitySettings) -> Self {
        Self {
            enabled: settings.otlp.enabled,
            traces: settings.tracing_enabled,
            endpoint: settings.otlp.effective_endpoint(),
            protocol: settings.otlp.protocol.clone(),
            timeout: Duration::from_secs(settings.otlp.timeout_secs),
            sample_ratio: settings.sampling.ratio,
            always_sample_errors: settings.sampling.always_sample_errors,
        }
    }

    #[cfg(test)]
    pub(crate) fn disabled() -> Self {
        Self {
            enabled: false,
            traces: false,
            endpoint: String::new(),
            protocol: "http/protobuf".to_owned(),
            timeout: Duration::from_secs(10),
            sample_ratio: 1.0,
            always_sample_errors: true,
        }
    }

    /// Builds the export layers and the pipelines that back them.
    pub(crate) fn build<S>(
        &self,
        service_name: &str,
        span_filter: tracing_subscriber::EnvFilter,
    ) -> Result<(ExportLayers<S>, Option<Providers>), ObservabilityError>
    where
        S: Subscriber + for<'a> LookupSpan<'a> + Send + Sync,
    {
        if !self.enabled {
            return Ok((Vec::new(), None));
        }
        let span_filter = span_filter;

        let resource = Resource::builder()
            .with_service_name(service_name.to_string())
            .build();

        let mut layers: ExportLayers<S> = Vec::new();

        let tracer_provider = if self.traces {
            let exporter = self.span_exporter()?;
            let provider = SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(resource.clone())
                .with_sampler(self.sampler())
                .build();
            // Its own filter, not `log_level`: logging at `warn` must not silence spans.
            layers.push(Box::new(
                tracing_opentelemetry::layer()
                    .with_tracer(provider.tracer(SCOPE))
                    .with_filter(span_filter),
            ));
            Some(provider)
        } else {
            drop(span_filter);
            None
        };

        let exporter = self.log_exporter()?;
        let logger_provider = SdkLoggerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(resource)
            .build();
        layers.push(Box::new(OpenTelemetryTracingBridge::new(&logger_provider)));

        Ok((
            layers,
            Some(Providers::new(tracer_provider, Some(logger_provider))),
        ))
    }

    /// Builds this plan's exporters and drops them unused; they connect lazily.
    pub(crate) fn check(&self) -> Result<(), ObservabilityError> {
        if !self.enabled {
            return Ok(());
        }
        if self.traces {
            drop(self.span_exporter()?);
        }
        drop(self.log_exporter()?);
        Ok(())
    }

    /// The head sampler, parent-based so an upstream decision is honoured, not re-rolled.
    fn sampler(&self) -> Sampler {
        // A tail decision, left to the collector's tail sampler.
        let _ = self.always_sample_errors;

        if self.sample_ratio >= 1.0 {
            Sampler::AlwaysOn
        } else if self.sample_ratio <= 0.0 {
            Sampler::AlwaysOff
        } else {
            Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(self.sample_ratio)))
        }
    }

    fn transport(&self) -> Result<Transport, ObservabilityError> {
        Transport::named(&self.protocol)
    }

    /// Over HTTP the signal's path under the base endpoint; over gRPC the endpoint as written.
    fn signal_endpoint(&self, transport: Transport, signal: &str) -> String {
        match transport {
            Transport::Http => format!("{}/v1/{signal}", self.endpoint.trim_end_matches('/')),
            #[cfg(feature = "otlp-grpc")]
            Transport::Grpc => self.endpoint.clone(),
        }
    }

    fn span_exporter(&self) -> Result<SpanExporter, ObservabilityError> {
        let transport = self.transport()?;
        let endpoint = self.signal_endpoint(transport, "traces");
        let builder = SpanExporter::builder();
        match transport {
            Transport::Http => builder
                .with_http()
                .with_endpoint(endpoint)
                .with_timeout(self.timeout)
                .build(),
            #[cfg(feature = "otlp-grpc")]
            Transport::Grpc => builder
                .with_tonic()
                .with_endpoint(endpoint)
                .with_timeout(self.timeout)
                .build(),
        }
        .map_err(|error| self.exporter_error("traces", &error))
    }

    fn log_exporter(&self) -> Result<LogExporter, ObservabilityError> {
        let transport = self.transport()?;
        let endpoint = self.signal_endpoint(transport, "logs");
        let builder = LogExporter::builder();
        match transport {
            Transport::Http => builder
                .with_http()
                .with_endpoint(endpoint)
                .with_timeout(self.timeout)
                .build(),
            #[cfg(feature = "otlp-grpc")]
            Transport::Grpc => builder
                .with_tonic()
                .with_endpoint(endpoint)
                .with_timeout(self.timeout)
                .build(),
        }
        .map_err(|error| self.exporter_error("logs", &error))
    }

    fn exporter_error(
        &self,
        signal: &'static str,
        error: &dyn std::fmt::Display,
    ) -> ObservabilityError {
        ObservabilityError::Exporter {
            signal,
            endpoint: self.endpoint.clone(),
            reason: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_over(protocol: &str) -> OtlpPlan {
        OtlpPlan {
            enabled: true,
            traces: true,
            endpoint: "http://127.0.0.1:4318/".to_owned(),
            protocol: protocol.to_owned(),
            ..OtlpPlan::disabled()
        }
    }

    /// Each signal has its own path under the base endpoint, or the collector answers 404.
    #[test]
    fn an_http_plan_sends_each_signal_to_its_path() {
        let plan = plan_over("http/protobuf");

        assert!(matches!(plan.transport(), Ok(Transport::Http)));
        assert_eq!(
            plan.signal_endpoint(Transport::Http, "traces"),
            "http://127.0.0.1:4318/v1/traces"
        );
        assert_eq!(
            plan.signal_endpoint(Transport::Http, "logs"),
            "http://127.0.0.1:4318/v1/logs"
        );
    }

    /// A TLS handshake opens with a handshake record (`0x16`).
    #[test]
    fn an_https_endpoint_is_spoken_to_over_tls() {
        use std::io::Read as _;
        use std::net::TcpListener;
        use std::sync::mpsc;

        use opentelemetry::trace::Tracer as _;

        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
        let port = listener.local_addr().expect("its address").port();
        let (first_byte, received) = mpsc::channel();
        std::thread::spawn(move || {
            let mut byte = [0_u8; 1];
            let read = listener
                .accept()
                .and_then(|(mut stream, _)| stream.read_exact(&mut byte));
            if read.is_ok() {
                let _ = first_byte.send(byte[0]);
            }
        });
        let plan = OtlpPlan {
            endpoint: format!("https://127.0.0.1:{port}"),
            timeout: Duration::from_secs(2),
            ..plan_over("http/protobuf")
        };
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(plan.span_exporter().expect("an https exporter builds"))
            .build();

        provider.tracer("test").in_span("an exported span", |_| ());

        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)),
            Ok(0x16),
            "no TLS handshake reached the collector"
        );
    }

    #[test]
    fn http_is_the_short_spelling_of_http_protobuf() {
        assert!(matches!(plan_over("HTTP").transport(), Ok(Transport::Http)));
    }

    #[cfg(not(feature = "otlp-grpc"))]
    #[test]
    fn grpc_without_the_feature_is_refused_naming_it() {
        let refused = plan_over("grpc").check();

        assert!(
            matches!(refused, Err(ObservabilityError::OtlpGrpcNotBuilt)),
            "{refused:?}"
        );
    }

    #[cfg(feature = "otlp-grpc")]
    #[test]
    fn grpc_with_the_feature_sends_to_the_endpoint_as_written() {
        let plan = plan_over("grpc");

        assert!(matches!(plan.transport(), Ok(Transport::Grpc)));
        assert_eq!(
            plan.signal_endpoint(Transport::Grpc, "traces"),
            "http://127.0.0.1:4318/"
        );
    }

    #[test]
    fn a_protocol_no_exporter_speaks_is_refused_naming_it() {
        let refused = plan_over("http/json").check();

        assert!(
            matches!(
                refused,
                Err(ObservabilityError::OtlpProtocol { ref protocol }) if protocol == "http/json"
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn a_full_ratio_samples_everything() {
        let plan = OtlpPlan {
            sample_ratio: 1.0,
            ..OtlpPlan::disabled()
        };
        assert!(matches!(plan.sampler(), Sampler::AlwaysOn));
    }

    /// Parent-based, or a trace crossing services is sampled independently at each hop.
    #[test]
    fn a_partial_ratio_defers_to_the_upstream_decision() {
        let plan = OtlpPlan {
            sample_ratio: 0.1,
            ..OtlpPlan::disabled()
        };
        assert!(matches!(plan.sampler(), Sampler::ParentBased(_)));
    }

    #[test]
    fn a_zero_ratio_samples_nothing() {
        let plan = OtlpPlan {
            sample_ratio: 0.0,
            ..OtlpPlan::disabled()
        };
        assert!(matches!(plan.sampler(), Sampler::AlwaysOff));
    }

    #[test]
    fn a_disabled_plan_builds_no_layers_and_no_pipelines() {
        let plan = OtlpPlan::disabled();
        let (layers, providers) = plan
            .build::<tracing_subscriber::Registry>(
                "test",
                tracing_subscriber::EnvFilter::new("info"),
            )
            .expect("a disabled plan cannot fail");

        assert!(layers.is_empty());
        assert!(providers.is_none());
    }

    #[test]
    fn an_enabled_plan_builds_a_tracer_when_tracing_is_on() {
        let plan = OtlpPlan {
            enabled: true,
            traces: true,
            endpoint: "http://127.0.0.1:4318".to_string(),
            ..OtlpPlan::disabled()
        };
        let (layers, providers) = plan
            .build::<tracing_subscriber::Registry>(
                "test",
                tracing_subscriber::EnvFilter::new("info"),
            )
            .expect("an enabled plan builds");

        let providers = providers.expect("an enabled plan reports its providers");
        assert!(
            providers.has_tracer(),
            "tracing_enabled was true and no tracer provider was built, so this process \
             exports logs and no spans — the shape that is indistinguishable from an \
             application that simply never instrumented anything"
        );
        assert_eq!(layers.len(), 2, "one trace layer and one log bridge");
    }

    #[test]
    fn an_enabled_plan_builds_no_tracer_when_tracing_is_off() {
        let plan = OtlpPlan {
            enabled: true,
            traces: false,
            endpoint: "http://127.0.0.1:4318".to_string(),
            ..OtlpPlan::disabled()
        };
        let (layers, providers) = plan
            .build::<tracing_subscriber::Registry>(
                "test",
                tracing_subscriber::EnvFilter::new("info"),
            )
            .expect("an enabled plan builds");

        let providers = providers.expect("an enabled plan reports its providers");
        assert!(
            !providers.has_tracer(),
            "traces were off and a tracer was built anyway"
        );
        assert_eq!(layers.len(), 1, "the log bridge alone");
    }
}
