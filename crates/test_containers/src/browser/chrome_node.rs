use std::time::Duration;

use serde_json::Value;
use tokio::sync::OnceCell;

use crate::container_recipe::ContainerRecipe;
use crate::start_container::start_container;
use crate::test_container_error::TestContainerError;

/// The pinned, multi-architecture headless Chrome release.
const IMAGE: &str = "chromedp/headless-shell";
const TAG: &str = "151.0.7922.109";

/// The DevTools port, on the host's loopback.
///
/// Fixed, because the container shares the host's network so pages a test serves at
/// `http://localhost:<port>` load as a secure origin.
const DEVTOOLS_PORT: u16 = 9322;

const NAME: &str = "fundamental-test-chrome";

const READY_BUDGET: Duration = Duration::from_secs(60);

static NODE: OnceCell<ChromeNode> = OnceCell::const_new();

/// A headless Chrome reachable over DevTools on the host's loopback only.
#[derive(Debug)]
pub struct ChromeNode {
    endpoint: String,
}

impl ChromeNode {
    /// Brings the machine's browser up on first use in this process and returns it.
    ///
    /// # Errors
    /// [`TestContainerError::Start`] when Docker will not produce the container, and
    /// [`TestContainerError::NotReady`] when it did and DevTools never answered.
    pub async fn shared() -> Result<&'static Self, TestContainerError> {
        NODE.get_or_try_init(|| async {
            start_container(&recipe()).await?;
            let endpoint = wait_until_ready().await?;
            Ok(Self { endpoint })
        })
        .await
    }

    /// The browser's DevTools WebSocket, `ws://127.0.0.1:9322/devtools/browser/<id>`.
    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

/// Polls DevTools' version document until it names the browser's WebSocket.
async fn wait_until_ready() -> Result<String, TestContainerError> {
    let version = format!("http://127.0.0.1:{DEVTOOLS_PORT}/json/version");
    let deadline = tokio::time::Instant::now() + READY_BUDGET;
    let mut last = String::from("never attempted");
    while tokio::time::Instant::now() < deadline {
        match reqwest::get(&version).await {
            Ok(response) if response.status().is_success() => {
                match response.json::<Value>().await {
                    Ok(body) => match body["webSocketDebuggerUrl"].as_str() {
                        Some(endpoint) => return Ok(endpoint.to_owned()),
                        None => last = "the version document names no WebSocket".to_owned(),
                    },
                    Err(error) => last = error.to_string(),
                }
            }
            Ok(response) => last = format!("version answered {}", response.status()),
            Err(error) => last = error.to_string(),
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(TestContainerError::NotReady {
        engine: IMAGE,
        endpoint: version,
        waited_secs: READY_BUDGET.as_secs(),
        detail: last,
    })
}

/// The arguments after the image's launcher win, so DevTools listens on loopback only.
fn recipe() -> ContainerRecipe {
    ContainerRecipe {
        name: NAME.to_owned(),
        image: IMAGE,
        tag: TAG,
        ports: Vec::new(),
        cmd: vec![
            "--remote-debugging-address=127.0.0.1",
            "--remote-debugging-port=9322",
        ],
        healthcheck: Vec::new(),
        startup_timeout: Duration::from_secs(90),
        files: Vec::new(),
        host_network: true,
    }
}

#[cfg(test)]
mod tests {
    use super::{DEVTOOLS_PORT, recipe};

    #[test]
    fn the_browser_listens_on_loopback_at_its_port_and_publishes_nothing() {
        let recipe = recipe();
        assert!(recipe.host_network);
        assert!(recipe.ports.is_empty());
        assert!(recipe.cmd.contains(&"--remote-debugging-address=127.0.0.1"));
        assert!(
            recipe
                .cmd
                .contains(&format!("--remote-debugging-port={DEVTOOLS_PORT}").as_str())
        );
    }
}
