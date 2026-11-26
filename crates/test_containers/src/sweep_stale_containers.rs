use std::collections::HashMap;

use bollard::Docker;
use bollard::query_parameters::{ListContainersOptionsBuilder, RemoveContainerOptionsBuilder};
use tracing::{info, warn};

use crate::container_recipe::{FINGERPRINT_LABEL, HARNESS_LABEL, HARNESS_VALUE};

/// Removes any container **of this name** whose recipe is not the one being asked for.
///
/// A changed fingerprint makes reuse miss into a 409 name conflict, so the old container must
/// go first. The name is compared exactly here, since Docker's filter is a substring match.
/// Never fails the caller.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) async fn sweep_stale_containers(name: &str, keep: &str) {
    let docker = match Docker::connect_with_defaults() {
        Ok(docker) => docker,
        Err(error) => {
            warn!(%error, "test containers: no Docker client for the sweep");
            return;
        }
    };

    let filters = HashMap::from([(
        "label".to_owned(),
        vec![format!("{HARNESS_LABEL}={HARNESS_VALUE}")],
    )]);
    let options = ListContainersOptionsBuilder::new()
        .all(true)
        .filters(&filters)
        .build();
    let ours = match docker.list_containers(Some(options)).await {
        Ok(containers) => containers,
        Err(error) => {
            warn!(%error, "test containers: could not list this harness's containers");
            return;
        }
    };

    for container in ours {
        let is_ours = container.names.as_ref().is_some_and(|names| {
            names
                .iter()
                .any(|candidate| candidate.trim_start_matches('/') == name)
        });
        if !is_ours {
            continue;
        }
        let current = container
            .labels
            .as_ref()
            .and_then(|labels| labels.get(FINGERPRINT_LABEL))
            .is_some_and(|fingerprint| fingerprint == keep);
        if current {
            continue;
        }
        let Some(id) = container.id else { continue };
        // `force`, because the replaced container is still running.
        let options = RemoveContainerOptionsBuilder::new()
            .force(true)
            .v(true)
            .build();
        match docker.remove_container(&id, Some(options)).await {
            Ok(()) => info!(
                container = %name,
                "test containers: removed a container built from an older recipe"
            ),
            Err(error) => warn!(
                container = %name, %error,
                "test containers: could not remove a stale container; \
                 the next acquire will fail on its name — remove it by hand"
            ),
        }
    }
}
