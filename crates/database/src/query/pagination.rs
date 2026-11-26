//! Limit/offset [`Pagination`] settings.

/// Pagination settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pagination {
    /// Maximum number of results.
    pub limit: u64,
    /// Number of results to skip.
    pub offset: u64,
}

impl Pagination {
    /// Creates pagination settings.
    pub fn new(limit: u64, offset: u64) -> Self {
        Self { limit, offset }
    }

    /// Creates pagination for a page (1-indexed).
    pub fn page(page: u64, per_page: u64) -> Self {
        let page = page.max(1);
        Self {
            limit: per_page,
            offset: (page - 1) * per_page,
        }
    }

    /// Returns the current page number (1-indexed).
    pub fn current_page(&self) -> u64 {
        self.offset.checked_div(self.limit).map_or(1, |q| q + 1)
    }
}

impl Default for Pagination {
    fn default() -> Self {
        Self {
            limit: 100,
            offset: 0,
        }
    }
}
