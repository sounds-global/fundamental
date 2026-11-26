use std::time::Duration;

use fundamental_nats::NatsConfig;
use tokio::sync::OnceCell;

use crate::container_name::NATS_05_AUTH;
use crate::container_recipe::ContainerRecipe;
use crate::nats::await_nats_ready::await_nats_ready;
use crate::nats::nats_server::{CLIENT_PORT, IMAGE, TAG, run_scoped_config};
use crate::nats::nats_user::NatsUser;
use crate::start_container::start_container;
use crate::test_container_error::TestContainerError;

/// Where the configuration is copied: the image root, since the image is `scratch`.
const CONFIG_PATH: &str = "/nats-auth.conf";

/// One account with JetStream, and the [`NatsUser`]s in it.
const CONFIG: &str = r#"port: 4222
jetstream: { store_dir: "/tmp/nats" }
accounts: {
  APP: {
    jetstream: enabled
    users: [
      { user: "provisioner", password: "provisioner-test-password" }
      { user: "app", password: "app-test-password",
        permissions: {
          publish: { allow: ["*.app.>", "_INBOX.>", "$JS.API.INFO"] }
          subscribe: { allow: ["*.app.>", "_INBOX.>"] }
        }
      }
      { user: "reader", password: "reader-test-password",
        permissions: {
          publish: { allow: ["$JS.API.INFO", "$JS.API.STREAM.INFO.>"] }
          subscribe: { allow: ["_INBOX.>"] }
        }
      }
    ]
  }
}
"#;

static SERVER: OnceCell<NatsAuthServer> = OnceCell::const_new();

/// A NATS server that refuses a client without a credential it knows.
///
/// It has no anonymous user, unlike every other server here.
#[derive(Debug)]
pub struct NatsAuthServer {
    url: String,
}

impl NatsAuthServer {
    /// Brings the server up on first use and returns it.
    ///
    /// # Errors
    /// [`TestContainerError::Start`] when Docker will not produce the container, and
    /// [`TestContainerError::NotReady`] when it did and the provisioner was never let in.
    pub async fn shared() -> Result<&'static Self, TestContainerError> {
        SERVER
            .get_or_try_init(|| async {
                let started = start_container(&recipe()).await?;
                let server = Self {
                    url: format!("nats://127.0.0.1:{}", started.ports[0]),
                };
                server.wait_until_ready().await?;
                Ok(server)
            })
            .await
    }

    /// The `nats://127.0.0.1:<port>` URL for this server.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// A client config with no credential, a first connect that fails at once, and every subject
    /// under this run's namespace.
    #[must_use]
    pub fn anonymous_config(&self) -> NatsConfig {
        run_scoped_config(&self.url)
    }

    /// A client config that connects as `user`.
    #[must_use]
    pub fn config_as(&self, user: NatsUser) -> NatsConfig {
        self.anonymous_config()
            .with_credentials(user.username(), user.password_text())
    }

    async fn wait_until_ready(&self) -> Result<(), TestContainerError> {
        await_nats_ready(&self.url, &self.config_as(NatsUser::Provisioner)).await
    }
}

fn recipe() -> ContainerRecipe {
    ContainerRecipe {
        name: NATS_05_AUTH.to_owned(),
        image: IMAGE,
        tag: TAG,
        ports: vec![CLIENT_PORT],
        cmd: vec!["-c", CONFIG_PATH],
        healthcheck: Vec::new(),
        startup_timeout: Duration::from_secs(60),
        files: vec![(CONFIG_PATH, CONFIG.as_bytes().to_vec())],
        host_network: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_configuration_carries_each_users_password() {
        for user in [NatsUser::Provisioner, NatsUser::App] {
            let entry = format!(
                "user: \"{}\", password: \"{}\"",
                user.username(),
                user.password_text()
            );
            assert!(
                CONFIG.contains(&entry),
                "{entry} is not in the server's configuration"
            );
        }
    }

    #[test]
    fn the_server_reads_the_file_it_is_given() {
        let recipe = recipe();
        assert_eq!(recipe.cmd, ["-c", CONFIG_PATH]);
        assert_eq!(recipe.files[0].0, CONFIG_PATH);
    }
}
