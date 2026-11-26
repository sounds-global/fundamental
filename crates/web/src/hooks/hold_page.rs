use std::future::Future;

use fundamental_core::AnyError;

use crate::PageContext;

/// Runs a blocking query's fetch and, on the server, records its failure on `page`.
#[doc(hidden)]
pub async fn hold_page<T>(
    page: Option<PageContext>,
    future: impl Future<Output = Result<T, AnyError>>,
) -> Result<T, AnyError> {
    let result = future.await;
    #[cfg(feature = "ssr")]
    if let (Err(error), Some(page)) = (&result, page) {
        page.refuse(error.clone());
    }
    #[cfg(not(feature = "ssr"))]
    let _ = page;
    result
}
