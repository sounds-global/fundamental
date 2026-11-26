//! Consistency: the [`Zookie`] token minted on write and honoured on read, and the
//! [`Consistency`] a check requests.

mod backend_tag;
mod consistency;
mod hybrid_clock;
mod hybrid_ts;
// The `Zookie` type lives in `zookie.rs`, one object per file.
#[allow(clippy::module_inception)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
mod zookie;

pub use backend_tag::BackendTag;
pub use consistency::Consistency;
pub use hybrid_clock::HybridClock;
pub use hybrid_ts::HybridTs;
pub use zookie::Zookie;
