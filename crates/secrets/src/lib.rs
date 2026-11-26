//! Remote secret-manager backend (HashiCorp Vault) for Fundamental.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::all)]

mod client;
mod error;
mod kv_cas;
mod kv_data;
mod kv_field;
mod kv_fields;
mod kv_path;
mod kv_version;
mod kv_written;
mod lease;
pub mod pki;
mod refreshed_secret;
mod store_answer;
pub mod transit;

pub use client::{KV_BODY_LIMIT, MIN_REFRESH_INTERVAL, VaultClient};
pub use error::{SecretError, SecretResult};
pub use kv_cas::KvCas;
pub use kv_data::{KV_FIELD_LIMIT, KV_KEY_LIMIT, KvData};
pub use kv_fields::KvFields;
pub use kv_path::KvPath;
pub use kv_version::KvVersion;
pub use kv_written::KvWritten;
pub use lease::{LeasedCredential, RenewerHandle, spawn_db_lease_renewer};
pub use refreshed_secret::RefreshedSecret;
pub use store_answer::StoreAnswer;
