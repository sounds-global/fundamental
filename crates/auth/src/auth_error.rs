//! Error type for principal authentication.

use fundamental_core::AnyError;
use fundamental_macros::Error;

use crate::{InvalidToken, UntrustedToken};

/// Errors raised while authenticating a principal, each with a stable `AUTH_0xx` code and status.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AuthError {
    /// A wrapped error that does not map onto a more specific variant.
    Other(AnyError),

    /// No token was presented.
    #[error(
        code = "AUTH_001",
        message = "No authentication token presented",
        status = 401
    )]
    MissingToken,

    /// The token was malformed, had a bad signature, or failed a claim check; never quotes the token.
    #[error(
        code = "AUTH_002",
        message = "Invalid authentication token: {0}",
        status = 401
    )]
    Invalid(InvalidToken),

    /// The token has expired.
    #[error(
        code = "AUTH_003",
        message = "Authentication token has expired",
        status = 401
    )]
    Expired,

    /// The token is valid but the principal is not trusted for this operation.
    #[error(
        code = "AUTH_004",
        message = "Principal is not authorized: {0}",
        status = 403
    )]
    Untrusted(UntrustedToken),

    /// The verifier could not be built from configuration.
    #[error(
        code = "AUTH_005",
        message = "Auth configuration error: {0}",
        status = 500
    )]
    Config(String),

    /// A JWKS endpoint could not be fetched or parsed.
    #[error(
        code = "AUTH_006",
        message = "JWKS error: {0}",
        status = 503,
        retryable
    )]
    Jwks(String),

    /// A token could not be signed.
    #[error(code = "AUTH_007", message = "Token signing failed: {0}", status = 500)]
    Signing(String),

    /// The token is valid but does not carry a scope the operation requires.
    #[error(
        code = "AUTH_008",
        message = "Insufficient scope: {scope} required",
        status = 403
    )]
    InsufficientScope {
        /// The required scope the principal does not hold.
        scope: String,
    },

    /// A token asked for a scope its issuer is not trusted to grant; refused, not trimmed.
    #[error(
        code = "AUTH_010",
        message = "The token asks for a scope its issuer may not grant",
        status = 403
    )]
    ScopeBeyondIssuerCeiling {
        /// The issuer whose ceiling was exceeded.
        issuer: String,
    },

    /// The credential cookie arrived with more than one distinct value; taking one would allow
    /// cookie tossing.
    #[error(
        code = "AUTH_009",
        message = "Ambiguous credential: the {name} cookie was presented with {count} different values",
        status = 401
    )]
    AmbiguousCredential {
        /// The configured cookie name, never a value.
        name: String,
        /// How many distinct values arrived under that name.
        count: usize,
    },

    /// The presented session handle names no live session: never issued, ended or expired, told apart
    /// to nobody.
    #[error(code = "AUTH_011", message = "The session has ended", status = 401)]
    SessionEnded,

    /// The session store could not be read; a `503` so an outage signs nobody out.
    #[error(
        code = "AUTH_012",
        message = "Sessions cannot be checked right now",
        developer_message = "the session store failed: {0}",
        status = 503,
        retryable
    )]
    SessionStoreUnavailable(String),

    /// The operating system's random source could not be read.
    #[error(
        code = "AUTH_013",
        message = "A session cannot be opened right now",
        developer_message = "the OS random source failed: {0}",
        status = 503,
        retryable
    )]
    EntropyUnavailable(String),
}
