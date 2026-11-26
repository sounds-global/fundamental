//! A single page of keyset-paginated YSQL results.

use super::SqlCursor;

/// A page of results with an opaque cursor for the next page.
#[derive(Debug)]
pub struct SqlPage<M> {
    /// The items in this page.
    pub items: Vec<M>,
    /// Cursor for the next page. `None` means this is the last page.
    pub next_page: Option<SqlCursor>,
    /// The first row's ordering tuple when the page was read from a cursor, for reading the page before.
    pub previous_page: Option<SqlCursor>,
}
