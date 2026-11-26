//! Configuration for paged queries.

/// Configuration for paged queries.
#[derive(Debug, Clone)]
pub struct PagingConfig {
    /// Number of rows per page.
    pub page_size: i32,

    /// Maximum total rows to fetch (0 = unlimited).
    pub max_rows: usize,

    /// Whether to automatically retry on transient errors.
    pub retry_on_error: bool,
}

impl Default for PagingConfig {
    fn default() -> Self {
        Self {
            page_size: 1000,
            max_rows: 0, // Unlimited
            retry_on_error: true,
        }
    }
}

impl PagingConfig {
    /// Creates a new paging configuration with the specified page size.
    pub fn new(page_size: i32) -> Self {
        Self {
            page_size,
            ..Default::default()
        }
    }

    /// Sets the maximum total rows to fetch.
    pub fn max_rows(mut self, max: usize) -> Self {
        self.max_rows = max;
        self
    }

    /// Disables automatic retry on errors.
    pub fn no_retry(mut self) -> Self {
        self.retry_on_error = false;
        self
    }
}
