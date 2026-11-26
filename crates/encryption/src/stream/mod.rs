//! A segmented envelope for content too large to hold whole: STREAM (`aead::stream::StreamBE32`)
//! over XChaCha20-Poly1305, each nonce carrying the segment's position and last flag.

mod content_key;
mod sealed_segment;
mod sealed_stream_error;
mod segment_opener;
mod segment_sealer;
mod stream_header;
mod stream_layout;
mod stream_refusal;

pub use content_key::ContentKey;
pub use sealed_segment::SealedSegment;
pub use sealed_stream_error::SealedStreamError;
pub use segment_opener::SegmentOpener;
pub use segment_sealer::SegmentSealer;
pub use stream_layout::StreamLayout;
pub use stream_refusal::StreamRefusal;
