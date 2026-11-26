use std::time::Duration;

use fundamental_core::secrets::SecretString;
use serde_json::json;
use tokio::sync::OnceCell;

use crate::container_recipe::ContainerRecipe;
use crate::start_container::start_container;
use crate::test_container_error::TestContainerError;

/// The pinned OpenBao image `fundamental_secrets`' Transit wire formats were recorded against.
const IMAGE: &str = "openbao/openbao";
const TAG: &str = "2.6.2";

const API_PORT: u16 = 8200;

/// The development root token; the server is in memory and never reachable off loopback.
const ROOT_TOKEN: &str = "fundamental-test-root";

const READY_BUDGET: Duration = Duration::from_secs(60);

static SERVER: OnceCell<OpenBaoServer> = OnceCell::const_new();

/// A development-mode OpenBao server for one test binary.
#[derive(Debug)]
pub struct OpenBaoServer {
    address: String,
    container: &'static str,
}

impl OpenBaoServer {
    /// Brings this binary's server up on first use and returns it.
    ///
    /// # Errors
    /// [`TestContainerError::Start`] when Docker will not produce the container, and
    /// [`TestContainerError::NotReady`] when it did and the server never reported healthy.
    pub async fn shared(container: &'static str) -> Result<&'static Self, TestContainerError> {
        let server = SERVER
            .get_or_try_init(|| async {
                let started = start_container(&recipe(container)).await?;
                let server = Self {
                    address: format!("http://127.0.0.1:{}", started.ports[0]),
                    container,
                };
                server.wait_until_ready().await?;
                Ok(server)
            })
            .await?;
        if server.container == container {
            Ok(server)
        } else {
            Err(TestContainerError::Provision {
                engine: "openbao",
                fixture: format!("a second OpenBao server named {container}"),
                detail: format!("this process already holds {}", server.container),
            })
        }
    }

    /// The server's address, `http://127.0.0.1:<port>`.
    #[must_use]
    pub fn address(&self) -> &str {
        &self.address
    }

    /// The development root token.
    #[must_use]
    pub fn root_token(&self) -> &'static str {
        ROOT_TOKEN
    }

    /// Mounts a Transit engine at `mount`, succeeding if one is already there.
    ///
    /// # Errors
    /// [`TestContainerError::Provision`] if the server refuses for any other reason.
    pub async fn enable_transit(&self, mount: &str) -> Result<(), TestContainerError> {
        self.enable_engine(mount, "transit").await
    }

    /// Mounts a secrets engine of `kind` at `mount`, succeeding if one is already there.
    ///
    /// # Errors
    /// [`TestContainerError::Provision`] if the server refuses for any other reason.
    pub async fn enable_engine(&self, mount: &str, kind: &str) -> Result<(), TestContainerError> {
        let response = self
            .post(&format!("sys/mounts/{mount}"), &json!({ "type": kind }))
            .await?;
        self.mounted(mount, response).await
    }

    /// Whether a mount request succeeded, or found the mount already there.
    async fn mounted(
        &self,
        mount: &str,
        response: reqwest::Response,
    ) -> Result<(), TestContainerError> {
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            provision(
                format!("mount {mount}"),
                format!("{status}; body unreadable: {error}"),
            )
        })?;
        if status.is_success() || body.contains("path is already in use") {
            Ok(())
        } else {
            Err(provision(
                format!("mount {mount}"),
                format!("{status}: {body}"),
            ))
        }
    }

    /// Writes the ACL policy `name` with the HCL `policy`.
    ///
    /// # Errors
    /// [`TestContainerError::Provision`] if the server refuses it.
    pub async fn write_policy(&self, name: &str, policy: &str) -> Result<(), TestContainerError> {
        self.expect_success(
            &format!("policy {name}"),
            self.post(
                &format!("sys/policies/acl/{name}"),
                &json!({ "policy": policy }),
            )
            .await?,
        )
        .await
        .map(drop)
    }

    /// Mints a token holding exactly `policies`, with no `default` and no root.
    ///
    /// # Errors
    /// [`TestContainerError::Provision`] if the server refuses, or answers without a token.
    pub async fn token_with_policies(
        &self,
        policies: &[&str],
    ) -> Result<SecretString, TestContainerError> {
        let body = self
            .expect_success(
                "a scoped token",
                self.post(
                    "auth/token/create",
                    &json!({ "policies": policies, "no_default_policy": true, "ttl": "1h" }),
                )
                .await?,
            )
            .await?;
        body.pointer("/auth/client_token")
            .and_then(serde_json::Value::as_str)
            .map(|token| SecretString::new(token.to_owned()))
            .ok_or_else(|| {
                provision(
                    "a scoped token".to_owned(),
                    "no auth.client_token in the answer".to_owned(),
                )
            })
    }

    /// Writes `data` at `path` under the development server's key-value engine (`secret/`, v2).
    ///
    /// # Errors
    /// [`TestContainerError::Provision`] if the server refuses it.
    pub async fn kv_put(
        &self,
        path: &str,
        data: &serde_json::Value,
    ) -> Result<(), TestContainerError> {
        self.kv_put_at("secret", path, data).await
    }

    /// Writes `data` at `path` under the key-value (version 2) engine mounted at `mount`.
    ///
    /// # Errors
    /// [`TestContainerError::Provision`] if the server refuses it.
    pub async fn kv_put_at(
        &self,
        mount: &str,
        path: &str,
        data: &serde_json::Value,
    ) -> Result<(), TestContainerError> {
        self.expect_success(
            &format!("{mount}/{path}"),
            self.post(&format!("{mount}/data/{path}"), &json!({ "data": data }))
                .await?,
        )
        .await
        .map(drop)
    }

    /// Mounts a key-value engine, version 2, at `mount`, succeeding if one is already there.
    ///
    /// # Errors
    /// [`TestContainerError::Provision`] if the server refuses for any other reason.
    pub async fn enable_kv(&self, mount: &str) -> Result<(), TestContainerError> {
        let response = self
            .post(
                &format!("sys/mounts/{mount}"),
                &json!({ "type": "kv", "options": { "version": "2" } }),
            )
            .await?;
        self.mounted(mount, response).await
    }

    /// The answer's JSON body, or a provisioning error naming `fixture`.
    async fn expect_success(
        &self,
        fixture: &str,
        response: reqwest::Response,
    ) -> Result<serde_json::Value, TestContainerError> {
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            provision(
                fixture.to_owned(),
                format!("{status}; body unreadable: {error}"),
            )
        })?;
        if !status.is_success() {
            return Err(provision(fixture.to_owned(), format!("{status}: {body}")));
        }
        if body.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        serde_json::from_str(&body)
            .map_err(|error| provision(fixture.to_owned(), format!("{status}; not JSON: {error}")))
    }

    /// Sends an authenticated `POST` to `/v1/{path}` with the root token, for arranging state.
    ///
    /// # Errors
    /// [`TestContainerError::Provision`] if the request cannot be sent.
    pub async fn post(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response, TestContainerError> {
        reqwest::Client::new()
            .post(format!("{}/v1/{path}", self.address))
            .header("X-Vault-Token", ROOT_TOKEN)
            .json(body)
            .send()
            .await
            .map_err(|error| provision(format!("POST {path}"), error.to_string()))
    }

    async fn wait_until_ready(&self) -> Result<(), TestContainerError> {
        let deadline = tokio::time::Instant::now() + READY_BUDGET;
        let mut last = String::from("never attempted");
        let health = format!("{}/v1/sys/health", self.address);
        while tokio::time::Instant::now() < deadline {
            match reqwest::get(&health).await {
                Ok(response) if response.status().is_success() => return Ok(()),
                Ok(response) => last = format!("health answered {}", response.status()),
                Err(error) => last = error.to_string(),
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err(TestContainerError::NotReady {
            engine: IMAGE,
            endpoint: health,
            waited_secs: READY_BUDGET.as_secs(),
            detail: last,
        })
    }
}

fn provision(fixture: String, detail: String) -> TestContainerError {
    TestContainerError::Provision {
        engine: "openbao",
        fixture,
        detail,
    }
}

/// Development mode with a fixed root token, listening on every interface in the container.
fn recipe(container: &'static str) -> ContainerRecipe {
    ContainerRecipe {
        name: container.to_owned(),
        image: IMAGE,
        tag: TAG,
        ports: vec![API_PORT],
        cmd: vec![
            "server",
            "-dev",
            "-dev-listen-address=0.0.0.0:8200",
            "-dev-root-token-id=fundamental-test-root",
        ],
        healthcheck: Vec::new(),
        startup_timeout: Duration::from_secs(90),
        files: Vec::new(),
        host_network: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container_name::OPENBAO_01_TRANSIT;

    #[test]
    fn the_recipe_root_token_is_the_one_the_fixture_reports() {
        let recipe = recipe(OPENBAO_01_TRANSIT);
        assert!(
            recipe
                .cmd
                .contains(&"-dev-root-token-id=fundamental-test-root")
        );
        assert_eq!(ROOT_TOKEN, "fundamental-test-root");
    }
}
