use fundamental_test_containers::browser::ChromeNode;
use serde_json::json;
use tracing::info;

use crate::connection::Connection;
use crate::{Page, TestBrowserError};

/// The machine's test browser, reached over DevTools.
#[derive(Clone)]
pub struct Browser {
    connection: Connection,
}

impl Browser {
    /// Starts or finds the pinned headless Chrome and connects to it.
    ///
    /// # Errors
    /// [`TestBrowserError::Unavailable`] when Docker cannot produce the browser or DevTools does not answer.
    pub async fn connect() -> Result<Self, TestBrowserError> {
        let node = ChromeNode::shared()
            .await
            .map_err(|error| TestBrowserError::Unavailable {
                detail: error.to_string(),
            })?;
        let connection = Connection::open(node.endpoint()).await?;
        let version = connection
            .call(None, "Browser.getVersion", json!({}))
            .await?;
        info!(product = %version["product"], "test browser connected");
        Ok(Self { connection })
    }

    /// A new page in a browser context of its own: its own cookies, storage and authenticators.
    ///
    /// # Errors
    /// [`TestBrowserError::Protocol`] when the browser refuses to make one.
    pub async fn page(&self) -> Result<Page, TestBrowserError> {
        Page::open(self.connection.clone()).await
    }
}
