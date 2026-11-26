use std::path::{Path, PathBuf};

use tracing::{debug, info, instrument};

use crate::machine_files::{LinuxTools, MACOS_DAEMON, MachineFiles, UNIT_NAME};
use crate::tool::run;
use crate::{
    Corefile, LocalError, LocalState, MachineOwner, MachineResolver, Platform, Privileged,
    RegistryStore,
};

const RESOLVECTL: &[&str] = &["/usr/bin/resolvectl", "/bin/resolvectl"];

/// `fun local setup`: the one privileged step a machine needs, after which nothing costs `sudo`.
#[derive(Debug, Clone, Copy)]
pub struct MachineSetup;

/// What `setup` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupReport {
    /// The platform it set up.
    pub platform: Platform,
    /// Every file it wrote as root.
    pub written: Vec<PathBuf>,
}

impl MachineSetup {
    /// Routes `.test`, and nothing else, to the machine resolver, records the owner, and starts the resolver.
    ///
    /// # Errors
    ///
    /// [`LocalError::RunAsRoot`] (`LOCAL_017`) under `sudo`; [`LocalError::NotMachineOwner`]
    /// (`LOCAL_018`) on another user's machine; [`LocalError::UnsupportedPlatform`]
    /// (`LOCAL_013`) on Linux without systemd-resolved, writing nothing;
    /// [`LocalError::ToolFailed`] (`LOCAL_012`) when a step fails;
    /// [`LocalError::ResolverNotRouted`] (`LOCAL_004`) when the route is not in place after.
    #[instrument(skip_all)]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn run(store: &RegistryStore) -> Result<SetupReport, LocalError> {
        let (platform, files) = Self::plan()?;
        let mkdir = Privileged::tool(&["/usr/bin/mkdir", "/bin/mkdir"])?;
        let mut written = Vec::new();
        for (path, contents) in files.files() {
            if let Some(parent) = path.parent() {
                Privileged::run(&mkdir, &["-p", &parent.display().to_string()], None)?;
            }
            Privileged::write(path, contents)?;
            written.push(path.clone());
        }
        match platform {
            Platform::Linux => {
                let systemctl = Privileged::tool(&["/usr/bin/systemctl", "/bin/systemctl"])?;
                Privileged::run(&systemctl, &["daemon-reload"], None)?;
                Privileged::run(&systemctl, &["enable", UNIT_NAME], None)?;
                Privileged::run(&systemctl, &["restart", UNIT_NAME], None)?;
                if Path::new("/run/NetworkManager").is_dir() {
                    Privileged::run(&systemctl, &["reload", "NetworkManager"], None)?;
                }
                let resolvectl = Privileged::tool(RESOLVECTL)?.display().to_string();
                let domains = run(&resolvectl, &["domain", Platform::ROUTING_LINK], None)?;
                let servers = run(&resolvectl, &["dns", Platform::ROUTING_LINK], None)?;
                if !domains.contains("~test")
                    || !servers.contains(&Platform::ROUTING_ADDRESS.to_string())
                {
                    return Err(LocalError::ResolverNotRouted {
                        name: format!("the {} link ({domains}{servers})", Platform::ROUTING_LINK),
                    });
                }
            }
            Platform::MacOs => {
                let launchctl = Privileged::tool(&["/bin/launchctl"])?;
                match Privileged::run(&launchctl, &["bootout", "system", MACOS_DAEMON], None) {
                    Ok(_) => debug!(daemon = MACOS_DAEMON, "the earlier daemon was unloaded"),
                    Err(error) => {
                        debug!(daemon = MACOS_DAEMON, %error, "no earlier daemon to unload; bootstrap decides");
                    }
                }
                Privileged::run(&launchctl, &["bootstrap", "system", MACOS_DAEMON], None)?;
            }
        }
        let registry = store.read()?;
        MachineResolver::write(store.state(), &Corefile::render(&registry, platform))?;
        MachineResolver::ensure_running(store.state(), platform)?;
        info!(?platform, files = written.len(), "local.setup");
        Ok(SetupReport { platform, written })
    }

    /// The files `run` would write as root, writing nothing: what `fun local setup --print` shows.
    ///
    /// # Errors
    ///
    /// [`LocalError::RunAsRoot`] (`LOCAL_017`), [`LocalError::NotMachineOwner`] (`LOCAL_018`),
    /// [`LocalError::UnsupportedPlatform`] (`LOCAL_013`), and [`LocalError::ToolFailed`]
    /// (`LOCAL_012`) when a tool the unit runs is not root's own.
    pub fn plan() -> Result<(Platform, MachineFiles), LocalError> {
        let euid = Privileged::effective_uid();
        Privileged::refuse_root(euid)?;
        MachineOwner::check(euid)?;
        let platform = Platform::current()?;
        let state = LocalState::for_user()?;
        let files = match platform {
            Platform::Linux => {
                let tools = LinuxTools {
                    ip: Privileged::tool(&["/usr/sbin/ip", "/sbin/ip", "/usr/bin/ip", "/bin/ip"])?,
                    resolvectl: Privileged::tool(RESOLVECTL)?,
                };
                MachineFiles::linux(
                    euid,
                    state.dir(),
                    &tools,
                    Path::new("/run/NetworkManager").is_dir(),
                )
            }
            Platform::MacOs => MachineFiles::macos(euid, state.dir()),
        };
        Ok((platform, files))
    }

    /// Whether this machine has been set up: its route is in place.
    #[must_use]
    pub fn done(platform: Platform) -> bool {
        MachineResolver::routed(platform)
    }

    /// The state directory `setup` is run against.
    ///
    /// # Errors
    ///
    /// As [`LocalState::for_user`].
    pub fn state() -> Result<LocalState, LocalError> {
        LocalState::for_user()
    }
}
