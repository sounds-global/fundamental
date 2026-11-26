use std::fs;
use std::os::unix::fs::{PermissionsExt, chown};
use std::path::Path;

use fundamental_core::fs::replace_file;
use rustix::process::getegid;
use tracing::{debug, info, instrument, warn};

use crate::caddyfile::EDGE_DIR;
use crate::tool::run;
use crate::{
    Caddyfile, InstanceFacts, InstanceKey, LocalError, LocalState, LoopbackPool, Platform,
};

/// One instance's edge: pinned Caddy in its own container, serving the instance's origins on
/// its address and port 443.
///
/// Only `NET_BIND_SERVICE` is kept, the root filesystem is read-only, and only the instance's
/// `edge/` is mounted, read-only, through group permissions. An edge evicts only edges of its own pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalEdge {
    pool: LoopbackPool,
}

/// The label holding the digest of what an edge container was started with.
const SERVES: &str = "fundamental.local.edge";
/// The label naming the project an edge container belongs to.
const PROJECT: &str = "fundamental.local.project";
/// The label naming the instance's slot, `main` or `lane-<label>`.
const SLOT: &str = "fundamental.local.slot";
/// The label naming the loopback address an edge serves on.
const ADDRESS: &str = "fundamental.local.address";

/// What [`LocalEdge::write`] left for the container: the digest it must serve and the group that may read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EdgeFiles {
    digest: String,
    group: u32,
}

impl LocalEdge {
    /// Caddy 2.11.4, pinned by digest so a tag moved upstream cannot change what serves.
    pub const IMAGE: &'static str = "caddy:2.11.4-alpine@sha256:6aeddd44c3078b0f9a35206472a11420648a79c184603ef95957d0a20044cb2b";

    /// The machine's edges, on `127.0.0.x`.
    pub const MACHINE: Self = Self::in_pool(LoopbackPool::MACHINE);

    /// The edges of `pool`.
    #[must_use]
    pub const fn in_pool(pool: LoopbackPool) -> Self {
        Self { pool }
    }

    /// The container of the instance `key`, its parts joined by `_`, which no project name or slot holds.
    #[must_use]
    pub fn container(self, key: &InstanceKey) -> String {
        format!("{}{}_{}", self.prefix(), key.project(), key.slot())
    }

    /// What every container name of the pool starts with.
    fn prefix(self) -> String {
        if self.pool == LoopbackPool::MACHINE {
            "fundamental-local-edge_".to_owned()
        } else {
            format!("fundamental-local-edge-{}_", self.pool.block())
        }
    }

    /// Writes `caddyfile` and a copy of the instance's leaf into its edge directory, atomically,
    /// readable by the developer's group only.
    ///
    /// # Errors
    ///
    /// [`LocalError::Io`] when the leaf cannot be read, or a file cannot be written or given
    /// its group and mode.
    pub(crate) fn write(
        state: &LocalState,
        facts: &InstanceFacts,
        caddyfile: &Caddyfile,
    ) -> Result<EdgeFiles, LocalError> {
        let group = getegid().as_raw();
        let dir = state.edge_dir(&facts.key);
        state.private_dir(&dir)?;
        Self::share(&dir, group, 0o750)?;
        let leaf =
            fs::read(&facts.tls).map_err(|error| LocalError::io(&facts.tls, "read", error))?;
        for (name, bytes) in [
            ("Caddyfile", caddyfile.as_str().as_bytes()),
            ("tls.pem", leaf.as_slice()),
        ] {
            let path = dir.join(name);
            replace_file(&path, bytes).map_err(|error| LocalError::io(&path, "written", error))?;
            Self::share(&path, group, 0o640)?;
        }
        Ok(EdgeFiles {
            digest: Self::files_digest(caddyfile, &leaf),
            group,
        })
    }

    /// Validates the instance's configuration with the pinned Caddy, then starts the edge unless
    /// one already runs exactly `files` with exactly these arguments.
    ///
    /// That is read from the container's label, never remembered, so an interrupted `up` is
    /// finished by the next. Other edges labelled with this address are removed first.
    ///
    /// # Errors
    ///
    /// [`LocalError::ToolFailed`] (`LOCAL_012`) when Caddy refuses the configuration or Docker
    /// cannot start it; the running edge is left as it was when validation fails.
    #[instrument(skip_all, fields(container = %self.container(&facts.key)))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) fn ensure(
        self,
        state: &LocalState,
        facts: &InstanceFacts,
        files: &EdgeFiles,
        platform: Platform,
    ) -> Result<(), LocalError> {
        let container = self.container(&facts.key);
        let dir = state.edge_dir(&facts.key);
        let config = format!("{EDGE_DIR}/Caddyfile");
        let mut start = Self::start_args(facts, &dir, files.group, platform, &config);
        // After `-d`, where it always stood: a machine edge keeps the digest it was started with.
        start.splice(2..2, ["--name".to_owned(), container.clone()]);
        let digest = Self::digest(&files.digest, &start);
        let serving = format!("label={SERVES}={digest}");
        if self.listed(
            &facts.key,
            &[
                "ps",
                "-q",
                "--filter",
                "status=running",
                "--filter",
                &serving,
            ],
        )? {
            debug!(%digest, "the edge already runs this configuration");
            return Ok(());
        }
        let mut validate = Self::base_args(&dir, files.group);
        validate.splice(1..1, ["--rm".to_owned()]);
        validate.extend([
            Self::IMAGE.to_owned(),
            "caddy".to_owned(),
            "validate".to_owned(),
            "--config".to_owned(),
            config.clone(),
            "--adapter".to_owned(),
            "caddyfile".to_owned(),
        ]);
        run("docker", &validate, None)?;
        self.remove(&facts.key)?;
        self.evict(facts)?;
        let mut labelled = start;
        labelled.splice(1..1, ["--label".to_owned(), format!("{SERVES}={digest}")]);
        run("docker", &labelled, None)?;
        info!(%container, %digest, address = %facts.address, "local.edge_recreated");
        Ok(())
    }

    /// `docker run` for the instance's edge, all but its name and the digest label.
    fn start_args(
        facts: &InstanceFacts,
        dir: &Path,
        group: u32,
        platform: Platform,
        config: &str,
    ) -> Vec<String> {
        let mut start = Self::base_args(dir, group);
        start.splice(
            1..1,
            [
                "-d".to_owned(),
                "--label".to_owned(),
                format!("{PROJECT}={}", facts.key.project()),
                "--label".to_owned(),
                format!("{SLOT}={}", facts.key.slot()),
                "--label".to_owned(),
                format!("{ADDRESS}={}", facts.address),
                "--restart".to_owned(),
                "unless-stopped".to_owned(),
            ],
        );
        match platform {
            Platform::Linux => start.extend(["--network".to_owned(), "host".to_owned()]),
            Platform::MacOs => start.extend([
                "-p".to_owned(),
                format!("{}:443:443", facts.address),
                "--add-host".to_owned(),
                "host.docker.internal:host-gateway".to_owned(),
            ]),
        }
        start.extend([
            Self::IMAGE.to_owned(),
            "caddy".to_owned(),
            "run".to_owned(),
            "--config".to_owned(),
            config.to_owned(),
            "--adapter".to_owned(),
            "caddyfile".to_owned(),
        ]);
        start
    }

    /// Removes every edge of this pool on the address of `facts` that is not the instance's own.
    fn evict(self, facts: &InstanceFacts) -> Result<(), LocalError> {
        let own = self.container(&facts.key);
        let prefix = self.prefix();
        let on_address = format!("label={ADDRESS}={}", facts.address);
        let listed = run(
            "docker",
            &[
                "ps",
                "-a",
                "--filter",
                &on_address,
                "--format",
                "{{.Names}}",
            ],
            None,
        )?;
        for other in listed
            .lines()
            .map(str::trim)
            .filter(|name| !name.is_empty() && *name != own && name.starts_with(&prefix))
        {
            run("docker", &["rm", "-f", other], None)?;
            warn!(container = %other, address = %facts.address, "local.edge_evicted: another instance's edge held this address");
        }
        Ok(())
    }

    /// Removes the instance's edge, if there is one.
    ///
    /// # Errors
    ///
    /// [`LocalError::ToolFailed`] (`LOCAL_012`) when Docker cannot be asked, or the container
    /// is still there afterwards.
    pub fn remove(self, key: &InstanceKey) -> Result<(), LocalError> {
        if self.present(key)? {
            let container = self.container(key);
            run("docker", &["rm", "-f", &container], None)?;
            info!(%container, "local.edge_removed");
        }
        Ok(())
    }

    /// Whether the edge of `key` is still a container on this machine.
    ///
    /// # Errors
    ///
    /// [`LocalError::ToolFailed`] (`LOCAL_012`) when Docker cannot be asked.
    pub fn present(self, key: &InstanceKey) -> Result<bool, LocalError> {
        self.listed(key, &["ps", "-aq"])
    }

    /// Whether `docker <command>` lists a container that is the instance's by name and by both labels.
    fn listed(self, key: &InstanceKey, command: &[&str]) -> Result<bool, LocalError> {
        let filters = [
            format!("name=^{}$", self.container(key)),
            format!("label={PROJECT}={}", key.project()),
            format!("label={SLOT}={}", key.slot()),
        ];
        let mut args: Vec<&str> = command.to_vec();
        for filter in &filters {
            args.extend(["--filter", filter]);
        }
        Ok(!run("docker", &args, None)?.trim().is_empty())
    }

    /// Gives `path` the group `group` and the mode `mode`.
    fn share(path: &Path, group: u32, mode: u32) -> Result<(), LocalError> {
        chown(path, None, Some(group))
            .map_err(|error| LocalError::io(path, "given the developer's group", error))?;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|error| LocalError::io(path, "made readable by the edge", error))
    }

    /// The digest of the edge's files: keyed BLAKE3 over the length-prefixed configuration, then the leaf.
    fn files_digest(caddyfile: &Caddyfile, leaf: &[u8]) -> String {
        let mut hasher = blake3::Hasher::new_derive_key("fundamental local edge 2026-10-09 files");
        let config = caddyfile.as_str().as_bytes();
        hasher.update(&(config.len() as u64).to_le_bytes());
        hasher.update(config);
        hasher.update(leaf);
        hasher.finalize().to_hex().to_string()
    }

    /// The digest an edge container is labelled with: its files' digest and every argument, length-prefixed.
    fn digest(files: &str, args: &[String]) -> String {
        let mut hasher = blake3::Hasher::new_derive_key("fundamental local edge 2026-10-09 served");
        for part in std::iter::once(files).chain(args.iter().map(String::as_str)) {
            hasher.update(&(part.len() as u64).to_le_bytes());
            hasher.update(part.as_bytes());
        }
        hasher.finalize().to_hex()[..32].to_owned()
    }

    /// `docker run` and every option an edge container runs with, but its name and network.
    fn base_args(dir: &Path, group: u32) -> Vec<String> {
        let mut args: Vec<String> = ["run", "--user", "0:0", "--group-add"]
            .iter()
            .map(|arg| (*arg).to_owned())
            .collect();
        args.push(group.to_string());
        args.extend(
            [
                "--read-only",
                "--tmpfs",
                "/data",
                "--tmpfs",
                "/config",
                "--cap-drop",
                "ALL",
                "--cap-add",
                "NET_BIND_SERVICE",
                "--security-opt",
                "no-new-privileges",
                "-v",
            ]
            .iter()
            .map(|arg| (*arg).to_owned()),
        );
        args.push(format!("{}:{EDGE_DIR}:ro", dir.display()));
        args
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::Path;

    use super::LocalEdge;
    use crate::{
        Caddyfile, InstanceFacts, InstanceKey, InstanceLabel, InstanceRole, LocalOrigins,
        LocalState, LoopbackAddress, LoopbackPool, Platform, ProjectName, RootFingerprint,
    };

    fn key(project: &str, role: InstanceRole) -> InstanceKey {
        InstanceKey::new(ProjectName::parse(project).expect("a name"), role)
    }

    fn lane(label: &str) -> InstanceRole {
        InstanceRole::Lane(InstanceLabel::parse(label).expect("a label"))
    }

    #[test]
    fn no_two_instances_share_an_edge_container() {
        let a_lane = LocalEdge::MACHINE.container(&key("a", lane("b-main")));
        let other_main = LocalEdge::MACHINE.container(&key("a-lane-b", InstanceRole::Main));
        assert_ne!(a_lane, other_main);
        assert_eq!(a_lane, "fundamental-local-edge_a_lane-b-main");
        assert_eq!(other_main, "fundamental-local-edge_a-lane-b_main");
    }

    #[test]
    fn a_test_edge_is_named_apart_from_every_machine_edge() {
        let main = key("a", InstanceRole::Main);
        let tests = LocalEdge::in_pool(LoopbackPool::TESTS).container(&main);
        assert_eq!(tests, "fundamental-local-edge-77_a_main");
        assert!(!tests.starts_with("fundamental-local-edge_"), "{tests}");
        assert_eq!(
            LocalEdge::MACHINE.container(&main),
            "fundamental-local-edge_a_main"
        );
    }

    #[test]
    fn a_change_in_how_the_edge_runs_is_a_different_edge() {
        let facts = |octet| InstanceFacts {
            suffixes: Vec::new(),
            key: key("acme", lane("fix-search")),
            address: LoopbackAddress::new(octet).expect("an address"),
            tls: "/s/tls.pem".into(),
            root: "/s/root.pem".into(),
            fingerprint: RootFingerprint::of_der(&[]),
        };
        let dir = Path::new("/s/edge");
        let args = |octet, group, platform| {
            LocalEdge::start_args(
                &facts(octet),
                dir,
                group,
                platform,
                "/etc/fundamental/Caddyfile",
            )
        };
        let base = LocalEdge::digest("files", &args(9, 1001, Platform::MacOs));
        assert_eq!(
            base,
            LocalEdge::digest("files", &args(9, 1001, Platform::MacOs))
        );
        for changed in [
            LocalEdge::digest("other files", &args(9, 1001, Platform::MacOs)),
            LocalEdge::digest("files", &args(10, 1001, Platform::MacOs)),
            LocalEdge::digest("files", &args(9, 1002, Platform::MacOs)),
            LocalEdge::digest("files", &args(9, 1001, Platform::Linux)),
        ] {
            assert_ne!(base, changed);
        }
        assert!(
            args(9, 1001, Platform::Linux)
                .contains(&"fundamental.local.address=127.0.0.9".to_owned())
        );
    }

    #[test]
    fn the_edge_holds_no_capability_but_binding_its_port() {
        let args = LocalEdge::base_args(Path::new("/s/edge"), 1001);
        let added: Vec<&str> = args
            .windows(2)
            .filter(|pair| pair[0] == "--cap-add")
            .map(|pair| pair[1].as_str())
            .collect();
        assert_eq!(added, ["NET_BIND_SERVICE"]);
        for expected in [
            "ALL",
            "no-new-privileges",
            "--read-only",
            "/s/edge:/etc/fundamental:ro",
        ] {
            assert!(args.iter().any(|arg| arg == expected), "{args:?}");
        }
        let group = args
            .iter()
            .position(|arg| arg == "--group-add")
            .expect("a group");
        assert_eq!(args.get(group + 1).map(String::as_str), Some("1001"));
    }

    #[test]
    fn the_edge_reads_a_group_copy_and_the_leaf_stays_private() {
        let dir = tempfile::tempdir().expect("a directory");
        let state = LocalState::at(dir.path().join("state"));
        let key = key("acme", lane("fix-search"));
        let tls = state.instance_dir(&key).join("tls.pem");
        state
            .private_dir(&state.instance_dir(&key))
            .expect("the instance");
        fs::write(&tls, "LEAF AND KEY").expect("a leaf");
        fs::set_permissions(&tls, fs::Permissions::from_mode(0o600)).expect("private");
        let facts = InstanceFacts {
            suffixes: Vec::new(),
            key: key.clone(),
            address: LoopbackAddress::new(9).expect("an address"),
            tls: tls.clone(),
            root: "/s/root.pem".into(),
            fingerprint: RootFingerprint::of_der(&[]),
        };
        let caddyfile = Caddyfile::render(&facts, &LocalOrigins::default(), Platform::Linux);
        let files = LocalEdge::write(&state, &facts, &caddyfile).expect("written");
        let edge = state.edge_dir(&key);
        let mode = |path: &Path| fs::metadata(path).expect("there").permissions().mode() & 0o777;
        assert_eq!(mode(&edge), 0o750);
        assert_eq!(mode(&edge.join("tls.pem")), 0o640);
        assert_eq!(mode(&edge.join("Caddyfile")), 0o640);
        assert_eq!(mode(&tls), 0o600, "the instance's own leaf is untouched");
        assert_eq!(
            fs::metadata(edge.join("tls.pem")).expect("there").gid(),
            files.group
        );
        assert_eq!(
            fs::read(edge.join("tls.pem")).expect("the copy"),
            b"LEAF AND KEY"
        );
        let again = LocalEdge::write(&state, &facts, &caddyfile).expect("written again");
        assert_eq!(files, again, "the same files are the same edge");
        fs::write(&tls, "A NEW LEAF").expect("a new leaf");
        let renewed = LocalEdge::write(&state, &facts, &caddyfile).expect("renewed");
        assert_ne!(files.digest, renewed.digest, "a new leaf is a new edge");
    }
}
