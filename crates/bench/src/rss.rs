use std::fs;
use std::path::Path;

use crate::BenchError;

/// How much memory this process holds resident, as the kernel counts it.
///
/// Read from `VmRSS` in kilobytes, not `statm` pages: the page size is not 4 KiB everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rss {
    bytes: u64,
}

impl Rss {
    /// This process's resident memory now.
    ///
    /// # Errors
    ///
    /// [`BenchError::Io`] when `/proc/self/status` cannot be read (not Linux), and
    /// [`BenchError::NoResidentSet`] when it has no `VmRSS` line.
    pub fn now() -> Result<Self, BenchError> {
        let path = Path::new("/proc/self/status");
        let status = fs::read_to_string(path).map_err(|error| BenchError::io(path, &error))?;
        Self::from_status(&status)
    }

    /// The resident memory a `/proc/<pid>/status` text states.
    ///
    /// # Errors
    ///
    /// [`BenchError::NoResidentSet`] when it has no readable `VmRSS` line.
    pub fn from_status(status: &str) -> Result<Self, BenchError> {
        status
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))
            .and_then(|value| value.trim().strip_suffix("kB"))
            .and_then(|kilobytes| kilobytes.trim().parse::<u64>().ok())
            .map(|kilobytes| Self {
                bytes: kilobytes * 1024,
            })
            .ok_or(BenchError::NoResidentSet)
    }

    /// The size in bytes.
    #[must_use]
    pub fn bytes(self) -> u64 {
        self.bytes
    }
}
