//! [`ReadScope`]: the handle every read of one evaluation goes through.

use super::WallTime;
use crate::zookie::HybridTs;

/// One evaluation's read handle: holding it is the proof that the reads taken
/// through it observe a single coherent state.
///
/// Opened and closed by [`TupleStore::evaluate`](super::TupleStore::evaluate), so no
/// decision is assembled from two states.
pub trait ReadScope: Send + Sync {
    /// The instant this scope's reads observe, stamped into the response zookie.
    fn ts(&self) -> HybridTs;

    /// The instant a caveat in this evaluation reads as `now`.
    ///
    /// Separate from [`ts`](Self::ts): an uncommitted scope stamps [`HybridTs::ZERO`], and a
    /// time-bounded grant judged at the epoch would admit forever.
    fn now(&self) -> WallTime;
}
