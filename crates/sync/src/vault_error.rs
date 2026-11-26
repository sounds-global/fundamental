//! Error type for the E2EE vault hub (the blinded-owner opaque-ciphertext path).

use fundamental_core::AnyError;
use fundamental_db::DatabaseError;
use fundamental_macros::Error;

/// Errors raised by the E2EE vault hub; every variant fails closed and carries no key material.
#[derive(Debug, Error)]
#[error_into(DatabaseError)]
#[non_exhaustive]
pub enum VaultError {
    /// A wrapped error that does not map onto a more specific variant.
    Other(AnyError),

    /// Sealing a vault row failed on the client (AEAD or entropy).
    #[error(
        code = "VAULT_001",
        message = "sealing a vault row failed",
        status = 500
    )]
    SealFailed,

    /// Opening a sealed vault row failed: a wrong key, tamper or an AAD mismatch, indistinguishably.
    #[error(
        code = "VAULT_002",
        message = "opening a vault row failed",
        status = 422
    )]
    UnsealFailed,

    /// A device could not prove ownership of the hub it addressed; which check failed is not said.
    #[error(
        code = "VAULT_003",
        message = "vault ownership proof failed",
        status = 401
    )]
    OwnerProofFailed,

    /// A sealed envelope was structurally invalid.
    #[error(code = "VAULT_004", message = "malformed vault envelope", status = 422)]
    MalformedEnvelope,

    /// A row's ciphertext exceeded the per-row size bound.
    #[error(
        code = "VAULT_005",
        message = "vault ciphertext exceeds the size bound ({bytes} > {max})",
        status = 413
    )]
    CiphertextTooLarge {
        /// The rejected blob's size.
        bytes: usize,
        /// The configured maximum.
        max: usize,
    },

    /// A vault model was served with no provisioned hub table.
    #[error(
        code = "VAULT_006",
        message = "no provisioned vault hub table `{table}`: add a migration for the \
                   `#[model(vault)]` model's e2ee hub",
        status = 500
    )]
    UnprovisionedVaultHub {
        /// The missing hub table.
        table: String,
    },
}
