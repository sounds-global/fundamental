use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use fundamental_core::fs::replace_file;
use ring::digest::{SHA256, digest};
use tracing::{debug, info, instrument, warn};

use crate::tool::run;
use crate::{Corefile, LocalError, LocalState, Platform};

/// The machine's one resolver: pinned CoreDNS in a container of no project, reading the registry's `Corefile`.
#[derive(Debug, Clone, Copy)]
pub struct MachineResolver;

/// The label naming the directory a resolver container serves.
const STATE_LABEL: &str = "fundamental.local.resolver_dir";

impl MachineResolver {
    /// The container's name.
    pub const CONTAINER: &'static str = "fundamental-local-dns";

    /// CoreDNS 1.12.0, pinned by digest so a tag moved upstream cannot change what answers.
    pub const IMAGE: &'static str = "coredns/coredns:1.12.0@sha256:40384aa1f5ea6bfdc77997d243aec73da05f27aed0c5e9d65bfa98933c519d97";

    /// Writes `corefile` into the resolver's directory as one atomic rename.
    ///
    /// The container mounts the directory, not the file, so a reload sees the renamed file.
    ///
    /// # Errors
    ///
    /// [`LocalError::Io`] when the directory or file cannot be written.
    #[instrument(skip_all)]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn write(state: &LocalState, corefile: &Corefile) -> Result<bool, LocalError> {
        let path = state.resolver_dir_readable()?.join("Corefile");
        if fs::read_to_string(&path).is_ok_and(|current| current == corefile.as_str()) {
            return Ok(false);
        }
        replace_file(&path, corefile.as_str().as_bytes())
            .map_err(|error| LocalError::io(&path, "written", error))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
            .map_err(|error| LocalError::io(&path, "made readable by the resolver", error))?;
        let digest = digest(&SHA256, corefile.as_str().as_bytes());
        let digest: String = digest.as_ref()[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        info!(%digest, "local.resolver_rendered");
        Ok(true)
    }

    /// Starts the hardened resolver unless it already serves `state`'s directory; one serving
    /// another directory is replaced.
    ///
    /// # Errors
    ///
    /// [`LocalError::ToolFailed`] (`LOCAL_012`) when Docker cannot start it.
    #[instrument]
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "readability baseline"
    )]
    pub fn ensure_running(state: &LocalState, platform: Platform) -> Result<(), LocalError> {
        let served = format!("{STATE_LABEL}={}", state.resolver_dir().display());
        let filter = format!("label={served}");
        let name = format!("name=^{}$", Self::CONTAINER);
        if !run(
            "docker",
            &["ps", "-q", "--filter", &name, "--filter", &filter],
            None,
        )?
        .trim()
        .is_empty()
        {
            return Ok(());
        }
        match run("docker", &["rm", "-f", Self::CONTAINER], None) {
            Ok(_) => debug!(
                container = Self::CONTAINER,
                "a stopped resolver container was removed"
            ),
            Err(LocalError::ToolFailed { stderr, .. }) if stderr.contains("No such container") => {
                debug!(container = Self::CONTAINER, "no earlier resolver container");
            }
            Err(error) => {
                warn!(container = Self::CONTAINER, %error, "an earlier resolver container could not be removed; starting one may fail on its name");
            }
        }
        let mount = format!("{}:/etc/coredns:ro", state.resolver_dir().display());
        let mut args: Vec<String> = [
            "run",
            "-d",
            "--name",
            Self::CONTAINER,
            "--restart",
            "unless-stopped",
            "--user",
            "0:0",
            "--read-only",
            "--cap-drop",
            "ALL",
            "--cap-add",
            "NET_BIND_SERVICE",
            "--security-opt",
            "no-new-privileges",
        ]
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect();
        args.extend(["--label".to_owned(), served, "-v".to_owned(), mount]);
        match platform {
            Platform::Linux => args.extend(["--network".to_owned(), "host".to_owned()]),
            Platform::MacOs => args.extend([
                "-p".to_owned(),
                "127.0.0.1:53:53/udp".to_owned(),
                "-p".to_owned(),
                "127.0.0.1:53:53/tcp".to_owned(),
            ]),
        }
        args.extend([
            Self::IMAGE.to_owned(),
            "-conf".to_owned(),
            "/etc/coredns/Corefile".to_owned(),
        ]);
        run("docker", &args, None)?;
        info!(container = Self::CONTAINER, "local.resolver_started");
        Ok(())
    }

    /// Whether the resolver's container is running.
    ///
    /// # Errors
    ///
    /// [`LocalError::ToolFailed`] (`LOCAL_012`) when Docker cannot be asked.
    pub fn running() -> Result<bool, LocalError> {
        let filter = format!("name=^{}$", Self::CONTAINER);
        Ok(!run("docker", &["ps", "-q", "--filter", &filter], None)?
            .trim()
            .is_empty())
    }

    /// Whether the routing link `fun local setup` creates exists.
    #[must_use]
    pub fn routed(platform: Platform) -> bool {
        match platform {
            Platform::Linux => Path::new("/sys/class/net")
                .join(Platform::ROUTING_LINK)
                .exists(),
            Platform::MacOs => Path::new("/etc/resolver/test").exists(),
        }
    }
}
