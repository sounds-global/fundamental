//! Result-set size validation for already-materialized results.
//!
//! The after-the-fact check: prefer [`CqlPool::fetch_all`](crate::CqlPool::fetch_all), which
//! stops at the bound.

use crate::{CqlError, CqlResult};

/// Checks whether a materialized result set exceeds the maximum unpaged size.
///
/// # Errors
///
/// Returns [`CqlError::InvalidQuery`] when `results` contains more than
/// `max` rows, naming the bounded read to use instead.
pub fn check_result_size<T>(results: &[T], max: usize) -> CqlResult<()> {
    if results.len() > max {
        Err(CqlError::InvalidQuery(format!(
            "Result set too large ({} rows). Maximum is {} rows without paging. \
             Use CqlPool::fetch_paged (or fetch_all, which bounds itself) for \
             large result sets.",
            results.len(),
            max
        )))
    } else {
        Ok(())
    }
}
