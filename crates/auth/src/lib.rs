//! Principal authentication for the [Fundamental](https://docs.rs/fundamental)
//! framework.
//!
//! [`PrincipalVerifier`] turns a presented token into an [`AuthenticatedPrincipal`]; the bundled
//! [`JwtVerifier`] verifies JWTs against the keys in
//! [`AuthSettings`](fundamental_config::settings::AuthSettings). It never records a token.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod auth_error;
mod claim_pins;
mod invalid_token;
mod jwks_endpoint;
mod jwks_key;
mod jwks_keys;
mod jwks_snapshot;
mod jwt_verifier;
mod multi_issuer_verifier;
mod principal_verifier;
mod record_refusal;
mod refused_jwk;
mod session_digest;
mod session_handle;
mod session_handle_key;
mod session_record;
mod session_store;
mod stored_session_verifier;
#[cfg(test)]
mod test_keys;
mod token_issuer;
mod trusted_issuer;
mod untrusted_token;

pub use auth_error::AuthError;
pub use invalid_token::InvalidToken;
pub use jwt_verifier::JwtVerifier;
pub use multi_issuer_verifier::MultiIssuerVerifier;
pub use principal_verifier::PrincipalVerifier;
pub use session_digest::SessionDigest;
pub use session_handle::SessionHandle;
pub use session_handle_key::SessionHandleKey;
pub use session_record::SessionRecord;
pub use session_store::SessionStore;
pub use stored_session_verifier::StoredSessionVerifier;
pub use token_issuer::TokenIssuer;
pub use trusted_issuer::TrustedIssuer;
pub use untrusted_token::UntrustedToken;

pub use fundamental_core::{AuthenticatedPrincipal, ScopeSet, UserId};
