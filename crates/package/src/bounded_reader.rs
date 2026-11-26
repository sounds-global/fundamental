//! [`BoundedReader`] — a reader that refuses to give more than its bound.

use std::io::{self, Read};

use crate::past_the_bound::PastTheBound;

/// Gives at most `limit` bytes of `inner`, and fails a read past them with [`PastTheBound`],
/// so a stream that decodes to more than it may is refused rather than cut short.
pub(crate) struct BoundedReader<R> {
    inner: R,
    left: u64,
}

impl<R: Read> BoundedReader<R> {
    pub(crate) const fn new(inner: R, limit: u64) -> Self {
        Self { inner, left: limit }
    }
}

impl<R: Read> Read for BoundedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let wanted = buffer
            .len()
            .min(usize::try_from(self.left.saturating_add(1)).unwrap_or(usize::MAX));
        let read = self.inner.read(&mut buffer[..wanted])?;
        if read as u64 > self.left {
            return Err(io::Error::other(PastTheBound));
        }
        self.left -= read as u64;
        Ok(read)
    }
}
