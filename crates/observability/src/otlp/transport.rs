use crate::ObservabilityError;

/// The wire `[observability.otlp] protocol` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transport {
    /// Protobuf bodies over HTTP, the specification's default.
    Http,
    /// gRPC, linked only with the `otlp-grpc` feature.
    #[cfg(feature = "otlp-grpc")]
    Grpc,
}

impl Transport {
    /// The transport a `protocol` setting names: `http/protobuf` (or `http`) or `grpc`, any case.
    pub(crate) fn named(protocol: &str) -> Result<Self, ObservabilityError> {
        let protocol = protocol.to_ascii_lowercase();
        match protocol.as_str() {
            "http/protobuf" | "http" => Ok(Self::Http),
            #[cfg(feature = "otlp-grpc")]
            "grpc" => Ok(Self::Grpc),
            #[cfg(not(feature = "otlp-grpc"))]
            "grpc" => Err(ObservabilityError::OtlpGrpcNotBuilt),
            _ => Err(ObservabilityError::OtlpProtocol { protocol }),
        }
    }
}
