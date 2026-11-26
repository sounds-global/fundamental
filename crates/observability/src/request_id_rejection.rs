use fundamental_macros::Error;

/// Why a value offered as a request id is not one.
///
/// Carries nothing of the refused value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum RequestIdRejection {
    /// Not a UUID in its 36-character hyphenated form.
    #[error(
        code = "OBS_009",
        message = "The request id is not a UUID",
        status = 400
    )]
    Malformed,

    /// Far longer than any UUID — the shape of a value pasted in, not an id.
    #[error(
        code = "OBS_010",
        message = "The request id is too long to be a UUID",
        status = 400
    )]
    Oversized,

    /// The nil or the max UUID, which identify nothing.
    #[error(
        code = "OBS_011",
        message = "The request id is a reserved UUID",
        status = 400
    )]
    Reserved,
}

impl RequestIdRejection {
    /// The class as a fixed, lower-case word — the value a log field records.
    #[must_use]
    pub const fn class(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::Oversized => "oversized",
            Self::Reserved => "reserved",
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
            (RequestIdRejection::Malformed, "OBS_009", "malformed"),
            (RequestIdRejection::Oversized, "OBS_010", "oversized"),
            (RequestIdRejection::Reserved, "OBS_011", "reserved"),
        ];
        for (rejection, code, class) in cases {
            assert_eq!(rejection.error_code(), code);
            assert_eq!(rejection.http_status_code(), 400);
            assert!(!rejection.is_retryable());
            assert_eq!(rejection.class(), class);
        }
    }
}
