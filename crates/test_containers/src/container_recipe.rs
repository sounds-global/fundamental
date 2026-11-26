use std::collections::HashMap;
use std::time::Duration;

use bollard::models::{HostConfig, PortBinding};
use testcontainers::core::{Healthcheck, IntoContainerPort, WaitFor};
use testcontainers::{ContainerRequest, GenericImage, ImageExt, ReuseDirective};

/// The label carrying a container's [`ContainerRecipe::fingerprint`].
///
/// Reuse matches only name and labels, so the label stops a bumped image tag being answered by
/// the old container.
pub(crate) const FINGERPRINT_LABEL: &str = "com.fundamental.test-recipe";

/// The label every container this harness starts carries, whatever the engine.
pub(crate) const HARNESS_LABEL: &str = "com.fundamental.test-harness";

pub(crate) const HARNESS_VALUE: &str = "fundamental_test_containers";

/// Everything that decides what a test container *is*, in one value.
///
/// [`request`](Self::request) and [`fingerprint`](Self::fingerprint) read the same fields, so
/// what a container runs cannot change without what it is looked up by.
#[derive(Debug, Clone)]
pub(crate) struct ContainerRecipe {
    /// The container's name, stable across runs so it can be reused.
    pub(crate) name: String,
    /// Image repository, e.g. `yugabytedb/yugabyte`.
    pub(crate) image: &'static str,
    /// Image tag, pinned rather than moving.
    pub(crate) tag: &'static str,
    /// Every port the harness publishes and dials.
    pub(crate) ports: Vec<u16>,
    /// The container's command.
    pub(crate) cmd: Vec<&'static str>,
    /// The in-container command that proves the engine is answering, or empty for none.
    ///
    /// Readiness is a query, never a log line; the caller's own poll is the gate on reuse.
    pub(crate) healthcheck: Vec<&'static str>,
    /// How long the first boot may take before `start()` gives up.
    pub(crate) startup_timeout: Duration,
    /// Files copied into the container before it starts, as `(path, contents)`; fingerprinted.
    pub(crate) files: Vec<(&'static str, Vec<u8>)>,
    /// Whether the container shares the host's network rather than publishing ports, so it can
    /// reach a server the test runs on the host's loopback.
    pub(crate) host_network: bool,
}

/// The only host address a test container's ports are published on.
///
/// A test node trusts whoever reaches it, so reaching it must mean being on this machine.
pub(crate) const PUBLISHED_ON: &str = "127.0.0.1";

impl ContainerRecipe {
    /// A stable hash of every field, as a short hex string.
    ///
    /// BLAKE3, so it is stable across builds and processes.
    pub(crate) fn fingerprint(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        // Length-prefixed so no two recipes hash alike across a field boundary.
        let mut field = |bytes: &[u8]| {
            hasher.update(&(bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
        };
        field(self.image.as_bytes());
        field(self.tag.as_bytes());
        for port in &self.ports {
            field(&port.to_le_bytes());
        }
        for argument in &self.cmd {
            field(argument.as_bytes());
        }
        for argument in &self.healthcheck {
            field(argument.as_bytes());
        }
        field(&self.startup_timeout.as_millis().to_le_bytes());
        for (path, contents) in &self.files {
            field(path.as_bytes());
            field(contents);
        }
        // Only when set, so the recipes that do not use it keep their fingerprint.
        if self.host_network {
            field(b"network=host");
        }
        hasher.finalize().to_hex()[..32].to_owned()
    }

    /// The `testcontainers` request this recipe describes.
    ///
    /// A fixed name with [`ReuseDirective::Always`] bounds the containers at one per name, since
    /// a `static` container is never dropped and nothing else removes one.
    pub(crate) fn request(&self) -> ContainerRequest<GenericImage> {
        // The wait condition belongs to the image, so it is set before the `ImageExt` builders.
        let mut image = GenericImage::new(self.image, self.tag);
        for port in &self.ports {
            image = image.with_exposed_port(port.tcp());
        }
        if !self.healthcheck.is_empty() {
            image = image.with_wait_for(WaitFor::healthcheck());
        }

        let published = self.ports.clone();
        let host_network = self.host_network;
        let mut request = image
            .with_container_name(self.name.clone())
            .with_reuse(ReuseDirective::Always)
            .with_label(FINGERPRINT_LABEL, self.fingerprint())
            .with_label(HARNESS_LABEL, HARNESS_VALUE)
            .with_cmd(self.cmd.iter().copied())
            .with_startup_timeout(self.startup_timeout)
            // Deliberately not fingerprinted, so shared containers are not recreated.
            .with_host_config_modifier(move |host| match host_network {
                true => host.network_mode = Some("host".to_owned()),
                false => publish_on_loopback(host, &published),
            });
        for (path, contents) in &self.files {
            request = request.with_copy_to(*path, contents.clone());
        }

        if self.healthcheck.is_empty() {
            return request;
        }
        request.with_health_check(
            Healthcheck::cmd(self.healthcheck.iter().copied())
                .with_interval(Duration::from_secs(1))
                .with_timeout(Duration::from_secs(5))
                .with_retries(60)
                .with_start_period(Duration::from_secs(2)),
        )
    }
}

/// Publishes exactly `ports`, each on [`PUBLISHED_ON`] at a port Docker picks.
fn publish_on_loopback(host: &mut HostConfig, ports: &[u16]) {
    host.publish_all_ports = Some(false);
    host.port_bindings = Some(
        ports
            .iter()
            .map(|port| {
                let binding = PortBinding {
                    host_ip: Some(PUBLISHED_ON.to_owned()),
                    host_port: Some(String::new()),
                };
                (format!("{port}/tcp"), Some(vec![binding]))
            })
            .collect::<HashMap<_, _>>(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe() -> ContainerRecipe {
        ContainerRecipe {
            name: "fundamental-test-example".to_owned(),
            image: "example/engine",
            tag: "1.2.3",
            ports: vec![1234],
            cmd: vec!["serve", "--quiet"],
            healthcheck: vec!["engine", "ping"],
            startup_timeout: Duration::from_secs(60),
            files: vec![("/engine.conf", b"listen: 1234".to_vec())],
            host_network: false,
        }
    }

    #[test]
    fn every_listed_port_is_published_on_loopback_and_nothing_else_is() {
        let mut host = HostConfig {
            publish_all_ports: Some(true),
            ..HostConfig::default()
        };
        publish_on_loopback(&mut host, &[5433, 9042]);

        assert_eq!(host.publish_all_ports, Some(false));
        let bindings = host.port_bindings.expect("the listed ports are bound");
        assert_eq!(bindings.len(), 2);
        for port in ["5433/tcp", "9042/tcp"] {
            let bound = bindings[port].as_deref().expect("bound");
            assert_eq!(bound.len(), 1);
            assert_eq!(bound[0].host_ip.as_deref(), Some(PUBLISHED_ON));
        }
    }

    #[test]
    fn the_same_recipe_fingerprints_the_same_way_every_time() {
        assert_eq!(recipe().fingerprint(), recipe().fingerprint());
    }

    #[test]
    fn every_field_that_changes_what_runs_changes_the_fingerprint() {
        type Mutation = Box<dyn Fn(&mut ContainerRecipe)>;

        let baseline = recipe().fingerprint();
        let mutations: Vec<Mutation> = vec![
            Box::new(|r| r.image = "example/other"),
            Box::new(|r| r.tag = "1.2.4"),
            Box::new(|r| r.ports = vec![4321]),
            Box::new(|r| r.cmd = vec!["serve"]),
            Box::new(|r| r.healthcheck = vec!["engine", "pong"]),
            Box::new(|r| r.startup_timeout = Duration::from_secs(61)),
            Box::new(|r| r.files = vec![("/engine.conf", b"listen: 4321".to_vec())]),
            Box::new(|r| r.files = vec![("/other.conf", b"listen: 1234".to_vec())]),
            Box::new(|r| r.host_network = true),
        ];
        for (index, mutate) in mutations.iter().enumerate() {
            let mut changed = recipe();
            mutate(&mut changed);
            assert_ne!(
                baseline,
                changed.fingerprint(),
                "mutation {index} left the fingerprint alone — a stale container would answer for it"
            );
        }
    }

    #[test]
    fn the_name_is_not_part_of_the_fingerprint() {
        // The name is how a container is found; the fingerprint is what it must be.
        let mut renamed = recipe();
        renamed.name = "fundamental-test-example-two".to_owned();
        assert_eq!(recipe().fingerprint(), renamed.fingerprint());
    }

    #[test]
    fn moving_a_character_across_a_field_boundary_changes_the_fingerprint() {
        let mut split_one = recipe();
        split_one.cmd = vec!["ab", "c"];
        let mut split_two = recipe();
        split_two.cmd = vec!["a", "bc"];
        assert_ne!(split_one.fingerprint(), split_two.fingerprint());
    }
}
