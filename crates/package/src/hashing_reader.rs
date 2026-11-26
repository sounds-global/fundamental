//! [`HashingReader`] — a reader that hashes what passes through it.

use std::io::{self, Read};

/// Hashes what is read through it.
pub(crate) struct HashingReader<R> {
    inner: R,
    hasher: blake3::Hasher,
}

impl<R: Read> HashingReader<R> {
    pub(crate) fn new(inner: R) -> Self {
        Self {
            inner,
            hasher: blake3::Hasher::new(),
        }
    }

    /// The BLAKE3 of what was read, as 64 lowercase hexadecimal digits.
    pub(crate) fn hex(&self) -> String {
        self.hasher.finalize().to_hex().to_string()
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..read]);
        Ok(read)
    }
}
