use fundamental_macros::Error;

/// Why a `traceparent` value is not a W3C trace context this process uses.
///
/// Carries nothing of the refused value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum TraceParentRejection {
    /// Longer than any trace context this process accepts.
    #[error(
        code = "OBS_004",
        message = "the traceparent is too long to be a trace context",
        status = 400
    )]
    Oversized,

    /// Not the `version-traceid-parentid-flags` shape of lowercase hex fields.
    #[error(
        code = "OBS_005",
        message = "the traceparent is not a W3C trace context",
        status = 400
    )]
    Malformed,

    /// Version `ff`, which the specification reserves as invalid.
    #[error(
        code = "OBS_006",
        message = "the traceparent uses the reserved version ff",
        status = 400
    )]
    UnsupportedVersion,

    /// A trace id of all zeroes, which names no trace.
    #[error(
        code = "OBS_007",
        message = "the traceparent names the all-zero trace id",
        status = 400
    )]
    ZeroTraceId,

    /// A parent id of all zeroes, which names no span.
    #[error(
        code = "OBS_008",
        message = "the traceparent names the all-zero parent id",
        status = 400
    )]
    ZeroParentId,
}

impl TraceParentRejection {
    /// The class as a fixed, lower-case word — the value a log field records.
    #[must_use]
    pub const fn class(self) -> &'static str {
        match self {
            Self::Oversized => "oversized",
            Self::Malformed => "malformed",
            Self::UnsupportedVersion => "unsupported-version",
            Self::ZeroTraceId => "zero-trace-id",
            Self::ZeroParentId => "zero-parent-id",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    #[test]
    fn every_class_has_a_stable_code_and_a_client_status() {
        let cases = [
            (TraceParentRejection::Oversized, "OBS_004", "oversized"),
            (TraceParentRejection::Malformed, "OBS_005", "malformed"),
            (
                TraceParentRejection::UnsupportedVersion,
                "OBS_006",
                "unsupported-version",
            ),
            (
                TraceParentRejection::ZeroTraceId,
                "OBS_007",
                "zero-trace-id",
            ),
            (
                TraceParentRejection::ZeroParentId,
                "OBS_008",
                "zero-parent-id",
            ),
        ];
        for (rejection, code, class) in cases {
            assert_eq!(rejection.error_code(), code);
            assert_eq!(rejection.http_status_code(), 400);
            assert!(!rejection.is_retryable());
            assert_eq!(rejection.class(), class);
        }
    }
}
