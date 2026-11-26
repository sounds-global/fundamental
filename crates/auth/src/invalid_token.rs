//! [`InvalidToken`] — why a presented token was refused before it proved anything.

use std::fmt;

use jsonwebtoken::errors::ErrorKind;

/// Why a token was refused as invalid: a closed set, so nothing read from the token is recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InvalidToken {
    /// Not a JWT: the wrong number of segments, bad base64, or bytes that are not UTF-8.
    Malformed,
    /// The header is not a JOSE header this verifier can read.
    MalformedHeader,
    /// The claims are not JSON of the shape a principal needs.
    MalformedClaims,
    /// The signature does not verify.
    BadSignature,
    /// The header names an algorithm the key set does not allow.
    AlgorithmNotAllowed,
    /// The `aud` claim names another service.
    WrongAudience,
    /// The `sub` claim is not one this verifier accepts.
    WrongSubject,
    /// The `nbf` claim is in the future.
    NotYetValid,
    /// A claim the verifier requires is absent.
    MissingClaim,
    /// The header names a key the key set does not hold.
    UnknownKey,
    /// The key could not be used to verify this token.
    Unverifiable,
    /// The token is not one this surface recognises.
    Unrecognised,
}

impl InvalidToken {
    /// The class as it is recorded: lower-case, one word per part.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::MalformedHeader => "malformed_header",
            Self::MalformedClaims => "malformed_claims",
            Self::BadSignature => "bad_signature",
            Self::AlgorithmNotAllowed => "algorithm_not_allowed",
            Self::WrongAudience => "wrong_audience",
            Self::WrongSubject => "wrong_subject",
            Self::NotYetValid => "not_yet_valid",
            Self::MissingClaim => "missing_claim",
            Self::UnknownKey => "unknown_key",
            Self::Unverifiable => "unverifiable",
            Self::Unrecognised => "unrecognised",
        }
    }

    /// Classifies a `jsonwebtoken` failure, or `None` for an expired token or another issuer's.
    #[must_use]
    pub(crate) fn from_kind(kind: &ErrorKind) -> Option<Self> {
        Some(match kind {
            ErrorKind::ExpiredSignature | ErrorKind::InvalidIssuer => return None,
            ErrorKind::InvalidToken | ErrorKind::Base64(_) | ErrorKind::Utf8(_) => Self::Malformed,
            ErrorKind::Json(_) | ErrorKind::InvalidClaimFormat(_) => Self::MalformedClaims,
            ErrorKind::InvalidSignature => Self::BadSignature,
            ErrorKind::InvalidAlgorithm
            | ErrorKind::InvalidAlgorithmName
            | ErrorKind::MissingAlgorithm => Self::AlgorithmNotAllowed,
            ErrorKind::InvalidAudience => Self::WrongAudience,
            ErrorKind::InvalidSubject => Self::WrongSubject,
            ErrorKind::ImmatureSignature => Self::NotYetValid,
            ErrorKind::MissingRequiredClaim(_) => Self::MissingClaim,
            _ => Self::Unverifiable,
        })
    }
}

impl fmt::Display for InvalidToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn a_parser_s_message_is_reduced_to_its_class() {
        let json = serde_json::from_str::<u8>("\"jan@example.nl\"").expect_err("not a u8");
        assert_eq!(
            InvalidToken::from_kind(&ErrorKind::Json(Arc::new(json))),
            Some(InvalidToken::MalformedClaims)
        );
        assert_eq!(
            InvalidToken::from_kind(&ErrorKind::InvalidClaimFormat("jan@example.nl".into())),
            Some(InvalidToken::MalformedClaims)
        );
        assert_eq!(
            InvalidToken::from_kind(&ErrorKind::MissingRequiredClaim("aud".into())),
            Some(InvalidToken::MissingClaim)
        );
    }

    #[test]
    fn expiry_and_issuer_are_not_invalid_tokens() {
        assert_eq!(InvalidToken::from_kind(&ErrorKind::ExpiredSignature), None);
        assert_eq!(InvalidToken::from_kind(&ErrorKind::InvalidIssuer), None);
    }

    #[test]
    fn a_class_displays_as_it_is_recorded() {
        assert_eq!(InvalidToken::BadSignature.to_string(), "bad_signature");
        assert_eq!(InvalidToken::UnknownKey.as_str(), "unknown_key");
    }
}
