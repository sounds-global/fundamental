use std::time::Duration;

use fundamental_nats::NatsConfig;
use tokio::sync::OnceCell;

use crate::container_name::NATS_06_NKEY;
use crate::container_recipe::ContainerRecipe;
use crate::nats::await_nats_ready::await_nats_ready;
use crate::nats::nats_server::{CLIENT_PORT, IMAGE, TAG, run_scoped_config};
use crate::nats::nats_user::NatsUser;
use crate::start_container::start_container;
use crate::test_container_error::TestContainerError;

const CONFIG_PATH: &str = "/nats-nkey.conf";

static SERVER: OnceCell<NatsNkeyServer> = OnceCell::const_new();

/// A NATS server whose application user is one NKey, besides the provisioner its readiness is
/// probed with; written at run time from the key's public half.
#[derive(Debug)]
pub struct NatsNkeyServer {
    url: String,
    public_key: String,
}

impl NatsNkeyServer {
    /// Brings up the server trusting the user NKey `public_key` on first use, and returns it.
    ///
    /// # Errors
    /// [`TestContainerError::Start`] when Docker will not produce the container,
    /// [`TestContainerError::NotReady`] when the provisioner was never let in, and
    /// [`TestContainerError::Provision`] when this process already started one for another key.
    pub async fn shared(public_key: &str) -> Result<&'static Self, TestContainerError> {
        let server = SERVER
            .get_or_try_init(|| async {
                let started = start_container(&recipe(public_key)).await?;
                let server = Self {
                    url: format!("nats://127.0.0.1:{}", started.ports[0]),
                    public_key: public_key.to_owned(),
                };
                server.wait_until_ready().await?;
                Ok(server)
            })
            .await?;
        if server.public_key == public_key {
            Ok(server)
        } else {
            Err(TestContainerError::Provision {
                engine: IMAGE,
                fixture: format!("an NKey server trusting {public_key}"),
                detail: format!(
                    "this process already holds one trusting {}",
                    server.public_key
                ),
            })
        }
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

    async fn wait_until_ready(&self) -> Result<(), TestContainerError> {
        let provisioner = self.anonymous_config().with_credentials(
            NatsUser::Provisioner.username(),
            NatsUser::Provisioner.password_text(),
        );
        await_nats_ready(&self.url, &provisioner).await
    }
}

/// One account: the NKey user, and the provisioner the readiness probe connects as.
fn config(public_key: &str) -> String {
    format!(
        "port: {CLIENT_PORT}\nauthorization: {{\n  users: [\n    {{ nkey: \"{public_key}\" }}\n    \
         {{ user: \"{}\", password: \"{}\" }}\n  ]\n}}\n",
        NatsUser::Provisioner.username(),
        NatsUser::Provisioner.password_text(),
    )
}

fn recipe(public_key: &str) -> ContainerRecipe {
    ContainerRecipe {
        name: NATS_06_NKEY.to_owned(),
        image: IMAGE,
        tag: TAG,
        ports: vec![CLIENT_PORT],
        cmd: vec!["-c", CONFIG_PATH],
        healthcheck: Vec::new(),
        startup_timeout: Duration::from_secs(60),
        files: vec![(CONFIG_PATH, config(public_key).into_bytes())],
        host_network: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "UAPZQH4MNJCOVEJFERB3NFSIROQ5RE7CGBEPKAZSB6QB7IQHBKXHZPVP";

    #[test]
    fn the_configuration_trusts_the_public_key_and_nothing_secret() {
        let written = config(KEY);
        assert!(
            written.contains(&format!("{{ nkey: \"{KEY}\" }}")),
            "{written}"
        );
        assert!(!written.contains("seed"), "{written}");
    }

    #[test]
    fn a_different_key_is_a_different_server() {
        let other = "UBQVJ4XBDSRUCAO6LBQKLIENJXLRBKVH5VAJZFP3MVFQS53VGBZ4RAOW";
        assert_ne!(recipe(KEY).fingerprint(), recipe(other).fingerprint());
    }
}
