//! Canonical bytes: one byte sequence per value, and digests that name exactly one thing.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod base64url32;
mod canonical_bytes;
mod canonical_error;
mod canonical_json;
mod canonical_member;
mod canonical_writer;
mod context_digest;
mod digest_context;
mod digest_purpose;
mod embedded_canonical;
mod from_canonical_slice;
mod from_digested_slice;
mod from_normal_slice;
mod limits;
mod opening_key;
mod profile;
mod reader;
mod safe_integer;
mod sha256_digest;
mod strict_deserializer;
mod to_canonical_vec;
mod unicode_data;
mod utf16_order;
mod validate_canonical;

pub use canonical_bytes::CanonicalBytes;
pub use canonical_error::CanonicalError;
pub use canonical_json::CanonicalJson;
pub use context_digest::ContextDigest;
pub use digest_context::DigestContext;
pub use digest_purpose::DigestPurpose;
pub use embedded_canonical::EmbeddedCanonical;
pub use from_canonical_slice::from_canonical_slice;
pub use from_digested_slice::from_digested_slice;
pub use from_normal_slice::from_normal_slice;
pub use limits::Limits;
pub use opening_key::OpeningKey;
pub use profile::Profile;
pub use safe_integer::SafeInteger;
pub use sha256_digest::Sha256Digest;
pub use to_canonical_vec::to_canonical_vec;
pub use unicode_data::UnicodeData;
pub use validate_canonical::validate_canonical;

/// What `#[derive(CanonicalJson)]` expands to; not a public interface.
#[doc(hidden)]
pub mod __private {
    pub use crate::canonical_member::CanonicalMember;
    pub use crate::canonical_writer::write_string;
}
