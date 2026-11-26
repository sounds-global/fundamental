use fundamental_macros::Error;

/// What can go wrong installing a process's telemetry.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ObservabilityError {
    /// A log-filter directive could not be parsed.
    #[error(
        code = "OBS_001",
        message = "log filter directive '{directive}' is invalid: {reason}",
        status = 500
    )]
    InvalidDirective {
        /// The directive as written.
        directive: String,
        /// The parser's complaint.
        reason: String,
    },

    /// A subscriber was already installed in this process.
    #[error(
        code = "OBS_002",
        message = "a tracing subscriber is already installed in this process",
        status = 500
    )]
    AlreadyInitialized,

    /// The app's configuration could not be loaded.
    #[error(transparent)]
    Config(#[from] fundamental_config::ConfigError),

    /// The OTLP exporter could not be constructed.
    #[error(
        code = "OBS_003",
        message = "could not build the OTLP {signal} exporter for '{endpoint}': {reason}",
        status = 500
    )]
    Exporter {
        /// Which signal — `traces` or `logs`.
        signal: &'static str,
        /// The endpoint that was being configured. Never carries credentials.
        endpoint: String,
        /// The exporter's complaint.
        reason: String,
    },

    /// `[observability.otlp]` asks for an export this binary was built without.
    #[error(
        code = "OBS_016",
        message = "'{service}' is configured to export over OTLP, but was built without the OTLP exporter (the `otlp` feature)",
        status = 500
    )]
    OtlpNotBuilt {
        /// The service whose settings asked for the export.
        service: String,
    },

    /// `[observability.otlp] protocol = "grpc"` in a build without the gRPC exporter.
    #[error(
        code = "OBS_017",
        message = "OTLP over gRPC was asked for, but this build has no gRPC exporter (the `otlp-grpc` feature); use protocol \"http/protobuf\" or build with `otlp-grpc`",
        status = 500
    )]
    OtlpGrpcNotBuilt,

    /// `[observability.otlp] protocol` names a wire no exporter here speaks.
    #[error(
        code = "OBS_018",
        message = "OTLP protocol '{protocol}' is not one this build exports over; use \"http/protobuf\" or \"grpc\"",
        status = 500
    )]
    OtlpProtocol {
        /// The protocol as written, in lower case.
        protocol: String,
    },
}
