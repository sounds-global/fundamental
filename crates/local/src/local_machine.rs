use std::fs;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use fundamental_core::Error as _;
use rustls_pki_types::CertificateDer;
use time::OffsetDateTime;
use tracing::field::{Empty, display};
use tracing::{info, info_span, instrument, warn};

use crate::{
    Caddyfile, CertificatePem, CertificateProof, Checkout, Corefile, DnsAnswer, InstanceFacts,
    InstanceKey, InstanceLabel, InstanceLeaf, InstanceNames, LocalDatabase, LocalEdge, LocalError,
    LocalOrigins, LocalProject, LocalState, LoopbackAddress, LoopbackPool, MachineOwner,
    MachineResolver, Platform, Privileged, Probe, ProjectName, ProjectRoot, Registry,
    RegistryStore, RootFingerprint, RootProfile, TrustPlan, TrustReport, TrustStore, UpReport,
};

/// This machine's local origins, and the verbs that change them.
///
/// An unmanaged machine (a test's) renders every artefact and touches nothing outside its state directory.
#[derive(Debug, Clone)]
pub struct LocalMachine {
    store: RegistryStore,
    platform: Platform,
    managed: bool,
    edges: bool,
    pool: LoopbackPool,
}

/// How long `forget` waits for the resolver to stop answering a forgotten name.
const RESOLVER_SETTLES: Duration = Duration::from_secs(10);

impl LocalMachine {
    /// The developer's machine: refuses to run as root or as a user other than its owner.
    ///
    /// # Errors
    ///
    /// [`LocalError::RunAsRoot`] (`LOCAL_017`), [`LocalError::NotMachineOwner`] (`LOCAL_018`),
    /// [`LocalError::UnsupportedPlatform`] (`LOCAL_013`).
    pub fn for_user() -> Result<Self, LocalError> {
        let euid = Privileged::effective_uid();
        Privileged::refuse_root(euid)?;
        MachineOwner::check(euid)?;
        Ok(Self {
            store: RegistryStore::new(LocalState::for_user()?),
            platform: Platform::current()?,
            managed: true,
            edges: true,
            pool: LoopbackPool::MACHINE,
        })
    }

    /// A machine whose state is `state`, rendering for `platform` and touching nothing outside it.
    #[must_use]
    pub const fn unmanaged(state: LocalState, platform: Platform) -> Self {
        Self {
            store: RegistryStore::new(state),
            platform,
            managed: false,
            edges: false,
            pool: LoopbackPool::MACHINE,
        }
    }

    /// Like [`unmanaged`](Self::unmanaged), but runs each instance's edge container, on
    /// [`LoopbackPool::TESTS`] so it never meets the machine's own instances.
    #[must_use]
    pub const fn with_edges(state: LocalState, platform: Platform) -> Self {
        Self {
            store: RegistryStore::new(state),
            platform,
            managed: false,
            edges: true,
            pool: LoopbackPool::TESTS,
        }
    }

    /// The edges of this machine's pool.
    const fn edge(&self) -> LocalEdge {
        LocalEdge::in_pool(self.pool)
    }

    /// The registry store.
    #[must_use]
    pub const fn store(&self) -> &RegistryStore {
        &self.store
    }

    /// The platform rendered for.
    #[must_use]
    pub const fn platform(&self) -> Platform {
        self.platform
    }

    /// Claims this checkout's instance and converges everything derived from it: root, leaf,
    /// resolver configuration, and on a managed machine the resolver.
    ///
    /// # Errors
    ///
    /// [`LocalError::ApexHeldElsewhere`] (`LOCAL_002`), [`LocalError::NoAddressLeft`]
    /// (`LOCAL_003`), [`LocalError::LeafCoversTooMuch`] (`LOCAL_006`),
    /// [`LocalError::InstanceHeldElsewhere`] (`LOCAL_009`), [`LocalError::ApexSetChanged`]
    /// (`LOCAL_015`), [`LocalError::RootMissing`] (`LOCAL_016`), [`LocalError::LockTimeout`]
    /// (`LOCAL_020`), and any I/O or tool failure.
    #[expect(
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub fn up(
        &self,
        project: &LocalProject,
        checkout: &Checkout,
        now: OffsetDateTime,
    ) -> Result<UpReport, LocalError> {
        let key = InstanceKey::new(project.name().clone(), checkout.role().clone());
        let span =
            info_span!("local.up", project = %key.project(), role = %key.role(), address = Empty);
        let _entered = span.enter();
        let state = self.store.state().clone();
        let (facts, der, issued, minted) = self.store.transact(|registry| {
            registry.register_project(project.name(), project.apexes())?;
            let (root, minted) = self.root_for_up(registry, project, now)?;
            let before: Vec<InstanceKey> =
                registry.instances().map(|(key, _)| key.clone()).collect();
            let address =
                registry.claim_instance_in(self.pool, &key, checkout.root(), checkout_exists)?;
            for released in before
                .iter()
                .filter(|other| **other != key && registry.instance(other).is_none())
            {
                self.give_back(&state, released)?;
            }
            let facts = facts_of(&state, &key, project, address, &root);
            let issued = self.converge_leaf(&root, &facts, project.origins(), now)?;
            self.converge_edge(project, &facts)?;
            self.converge_resolver(registry)?;
            Ok((facts, root.der().clone(), issued, minted))
        })?;
        span.record("address", display(facts.address));
        converge_database(project)?;
        let mut warnings = Vec::new();
        if self.managed {
            if !MachineResolver::routed(self.platform) {
                warnings.push(LocalError::ResolverNotRouted {
                    name: probe_name(&facts),
                });
            }
            match self.trust_in(project.name(), &der) {
                Ok(stores) => {
                    let missing: Vec<String> = stores
                        .into_iter()
                        .filter(|(_, held)| !held)
                        .map(|(store, _)| store.describe())
                        .collect();
                    if !missing.is_empty() {
                        warnings.push(LocalError::RootNotTrusted {
                            project: key.project().to_string(),
                            stores: missing.join(", "),
                        });
                    }
                }
                Err(error) => warnings.push(error),
            }
        }
        for warning in &warnings {
            warn!(code = %warning.error_code(), %warning, "local.up: a browser cannot reach the instance yet");
        }
        info!(address = %facts.address, issued, minted, "local.up");
        Ok(UpReport {
            facts,
            issued,
            minted,
            warnings,
        })
    }

    /// The facts of this checkout's instance, without changing anything.
    ///
    /// # Errors
    ///
    /// [`LocalError::InstanceNotUp`] (`LOCAL_021`) when the checkout holds no instance;
    /// [`LocalError::RootMissing`] (`LOCAL_016`) when its project has no root.
    pub fn facts(
        &self,
        project: &LocalProject,
        checkout: &Checkout,
    ) -> Result<InstanceFacts, LocalError> {
        let key = InstanceKey::new(project.name().clone(), checkout.role().clone());
        let registry = self.store.read()?;
        let entry = registry
            .instance(&key)
            .filter(|entry| entry.checkout() == checkout.root())
            .ok_or(LocalError::InstanceNotUp)?;
        let fingerprint = registry
            .project(project.name())
            .and_then(|project| project.root().copied())
            .ok_or(LocalError::InstanceNotUp)?;
        let root = ProjectRoot::load(self.store.state(), project.name(), &fingerprint)?;
        Ok(facts_of(
            self.store.state(),
            &key,
            project,
            entry.address(),
            &root,
        ))
    }

    /// Whether each store a browser on this machine reads holds exactly the instance's root.
    ///
    /// # Errors
    ///
    /// [`LocalError::RootMissing`] (`LOCAL_016`) when the root is gone, and
    /// [`LocalError::UnsupportedPlatform`] (`LOCAL_013`) when the stores cannot be listed.
    pub fn trusted(
        &self,
        project: &LocalProject,
        facts: &InstanceFacts,
    ) -> Result<Vec<(TrustStore, bool)>, LocalError> {
        let root = ProjectRoot::load(self.store.state(), project.name(), &facts.fingerprint)?;
        self.trust_in(project.name(), root.der())
    }

    /// Gives back this checkout's instance and asserts nothing of it is left, returning the
    /// address given back, or `None` when nothing held it.
    ///
    /// An instance another existing checkout holds is refused; files no entry names are removed.
    ///
    /// # Errors
    ///
    /// [`LocalError::InstanceHeldElsewhere`] (`LOCAL_009`) when another existing checkout holds
    /// the instance; [`LocalError::LeftBehind`] (`LOCAL_023`) naming what is still there; and
    /// any lock, I/O or tool failure.
    #[instrument(name = "local.forget", skip_all, fields(project = %project.name(), role = %checkout.role()))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn forget(
        &self,
        project: &LocalProject,
        checkout: &Checkout,
    ) -> Result<Option<LoopbackAddress>, LocalError> {
        let key = InstanceKey::new(project.name().clone(), checkout.role().clone());
        let state = self.store.state().clone();
        let released = self.store.transact(|registry| {
            let released = match registry.instance(&key) {
                Some(entry) if entry.checkout() == checkout.root() => registry.release(&key),
                Some(entry) if checkout_exists(entry.checkout()) => {
                    return Err(LocalError::InstanceHeldElsewhere {
                        instance: key.to_string(),
                        holder: entry.checkout().display().to_string(),
                    });
                }
                Some(entry) => {
                    info!(holder = %entry.checkout().display(), "local.forget: the holder's checkout is gone; its instance is given back");
                    registry.release(&key)
                }
                None => None,
            };
            self.give_back(&state, &key)?;
            self.converge_resolver(registry)?;
            Ok(released)
        })?;
        let mut left = Vec::new();
        if self.store.read()?.instance(&key).is_some() {
            left.push("its registry entry".to_owned());
        }
        if state.instance_dir(&key).exists() {
            left.push(state.instance_dir(&key).display().to_string());
        }
        if self.edges && self.edge().present(&key)? {
            left.push(format!(
                "the edge container {}",
                self.edge().container(&key)
            ));
        }
        let probe = project
            .apexes()
            .iter()
            .next()
            .map(|apex| key.role().suffix(apex).name(Some(PROBE_HOST)));
        if self.managed
            && released.is_some()
            && MachineOwner::serves(self.store.state())
            && let Some(probe) = probe
            && self.resolver_running()
            && !self.resolver_forgets(&probe)
        {
            left.push(format!("a resolver answer for {probe}"));
        }
        if !left.is_empty() {
            return Err(LocalError::LeftBehind {
                what: left.join(", "),
            });
        }
        let address = released.map(|entry| entry.address());
        info!(released = ?address, "local.forget");
        Ok(address)
    }

    /// Judges the root `trust` would install, before anything privileged runs, minting one when
    /// needed and refusing any but exactly one certificate of the minted profile for exactly the
    /// project's names, held by no other project.
    ///
    /// # Errors
    ///
    /// [`LocalError::ApexHeldElsewhere`] (`LOCAL_002`) when another project's root permits one
    /// of the names, in the registry or an installed anchor; [`LocalError::LeafCoversTooMuch`]
    /// (`LOCAL_006`) when the file is not exactly one certificate of the minted profile for
    /// exactly the project's names; [`LocalError::RootMissing`] (`LOCAL_016`) when the file is
    /// not the registered root; [`LocalError::UnsupportedPlatform`] (`LOCAL_013`) when the
    /// stores cannot be listed; and any lock or I/O failure.
    #[instrument(name = "local.trust_plan", skip_all, fields(project = %project.name()))]
    pub fn plan_trust(
        &self,
        project: &LocalProject,
        now: OffsetDateTime,
    ) -> Result<TrustPlan, LocalError> {
        let state = self.store.state().clone();
        let names = project.apexes().permitted();
        let (fingerprint, minted) = self.store.transact(|registry| {
            registry.register_project(project.name(), project.apexes())?;
            match self.trusted_root(registry, project, &names)? {
                Some(root) => Ok((root, false)),
                None => Ok((self.mint(registry, project, now)?.fingerprint(), true)),
            }
        })?;
        let path = ProjectRoot::certificate_path(&state, &fingerprint);
        let text =
            fs::read_to_string(&path).map_err(|error| LocalError::io(&path, "read", error))?;
        let der =
            CertificatePem::single(&text).map_err(|detail| LocalError::LeafCoversTooMuch {
                probe: "root-profile".to_owned(),
                detail: format!("{} {detail}", path.display()),
            })?;
        if RootFingerprint::of_der(&der) != fingerprint {
            return Err(LocalError::RootMissing {
                project: project.name().to_string(),
                fingerprint: fingerprint.to_string(),
                path: path.display().to_string(),
            });
        }
        let profile = RootProfile::exact(&der)?;
        if profile.permitted() != names.as_slice() {
            return Err(LocalError::LeafCoversTooMuch {
                probe: "root-profile".to_owned(),
                detail: format!(
                    "the root permits {:?}, and the project's names are {names:?}",
                    profile.permitted()
                ),
            });
        }
        let stores = TrustStore::every(self.platform)?;
        for store in &stores {
            store.refuse_overlap(project.name(), &names)?;
        }
        Ok(TrustPlan {
            project: project.name().clone(),
            der,
            fingerprint,
            permitted: names,
            stores,
            minted,
        })
    }

    /// Installs the root `plan` judged, re-encoded from its DER, in every store the plan names, reading each back.
    ///
    /// # Errors
    ///
    /// [`LocalError::ToolFailed`] (`LOCAL_012`) when a step fails;
    /// [`LocalError::RootNotTrusted`] (`LOCAL_005`) when a store does not read it back.
    #[instrument(name = "local.trust", skip_all, fields(project = %plan.project, fingerprint = %plan.fingerprint))]
    pub fn trust(&self, plan: &TrustPlan) -> Result<TrustReport, LocalError> {
        let mut stores = Vec::with_capacity(plan.stores.len());
        for store in &plan.stores {
            store.install(&plan.project, &plan.der)?;
            stores.push(store.describe());
        }
        Ok(TrustReport {
            fingerprint: plan.fingerprint,
            stores,
        })
    }

    /// The registered root `trust` can keep, or `None` when one must be minted.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn trusted_root(
        &self,
        registry: &Registry,
        project: &LocalProject,
        names: &[String],
    ) -> Result<Option<RootFingerprint>, LocalError> {
        let Some(entry) = registry.project(project.name()) else {
            return Ok(None);
        };
        let Some(fingerprint) = entry.root().copied() else {
            return Ok(None);
        };
        if entry.apexes() != project.apexes() {
            info!(registered = %entry.apexes(), declared = %project.apexes(), "local.trust: the apexes changed; minting a root for them");
            return Ok(None);
        }
        let root = match ProjectRoot::load(self.store.state(), project.name(), &fingerprint) {
            Ok(root) => root,
            Err(error @ LocalError::RootMissing { .. }) => {
                warn!(%error, "local.trust: the registered root is gone; minting another");
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let permitted = RootProfile::exact(root.der())?.permitted().to_vec();
        if permitted != names {
            warn!(
                ?permitted,
                ?names,
                "local.trust: the registered root permits other names; minting another"
            );
            return Ok(None);
        }
        Ok(Some(fingerprint))
    }

    fn trust_in(
        &self,
        project: &ProjectName,
        der: &CertificateDer<'_>,
    ) -> Result<Vec<(TrustStore, bool)>, LocalError> {
        Ok(TrustStore::every(self.platform)?
            .into_iter()
            .map(|store| {
                let held = store.holds(project, der);
                (store, held)
            })
            .collect())
    }

    fn root_for_up(
        &self,
        registry: &mut Registry,
        project: &LocalProject,
        now: OffsetDateTime,
    ) -> Result<(ProjectRoot, bool), LocalError> {
        let entry = registry.project(project.name()).cloned();
        let Some((entry, fingerprint)) = entry
            .as_ref()
            .and_then(|entry| entry.root().copied().map(|root| (entry, root)))
        else {
            return Ok((self.mint(registry, project, now)?, true));
        };
        if entry.apexes() != project.apexes() {
            return Err(LocalError::ApexSetChanged {
                project: project.name().to_string(),
                declared: project.apexes().to_string(),
                registered: entry.apexes().to_string(),
            });
        }
        let root = ProjectRoot::load(self.store.state(), project.name(), &fingerprint)?;
        let permitted = RootProfile::exact(root.der())?.permitted().join(", ");
        let names = project.apexes().permitted().join(", ");
        if permitted != names {
            return Err(LocalError::ApexSetChanged {
                project: project.name().to_string(),
                declared: names,
                registered: permitted,
            });
        }
        Ok((root, false))
    }

    fn mint(
        &self,
        registry: &mut Registry,
        project: &LocalProject,
        now: OffsetDateTime,
    ) -> Result<ProjectRoot, LocalError> {
        let root = ProjectRoot::mint(project.name(), project.apexes(), now)?;
        let other = project
            .apexes()
            .iter()
            .next()
            .map(|apex| format!("{}-elsewhere.test", apex.label()))
            .unwrap_or_default();
        let inside = project
            .apexes()
            .iter()
            .next()
            .map(|apex| format!("probe.{apex}"))
            .unwrap_or_default();
        CertificateProof::at(now).root(
            &root,
            &inside,
            &[
                Probe::Name("probe.example.com".to_owned()),
                Probe::Name(format!("probe.{other}")),
                Probe::Address(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            ],
        )?;
        root.store(self.store.state())?;
        registry.set_root(project.name(), project.apexes(), root.fingerprint())?;
        Ok(root)
    }

    fn converge_leaf(
        &self,
        root: &ProjectRoot,
        facts: &InstanceFacts,
        origins: &LocalOrigins,
        now: OffsetDateTime,
    ) -> Result<bool, LocalError> {
        // `*.<suffix>` covers one label in front, so an origin at `*.preview` needs its own wildcard.
        let names = origins.leaf_names(facts);
        let deeper = origins.deeper_names(facts);
        if InstanceLeaf::still_serves(&facts.tls, root, &names, now) {
            return Ok(false);
        }
        let leaf = InstanceLeaf::issue(root, &names, now)?;
        let own: Vec<String> = facts
            .suffixes
            .iter()
            .flat_map(|(_, suffix)| [suffix.to_string(), suffix.name(Some("probe"))])
            .chain(deeper.iter().map(|name| name.replacen('*', "probe", 1)))
            .collect();
        let neighbour = neighbour_of(facts);
        let refused: Vec<String> = facts
            .suffixes
            .iter()
            .flat_map(|(apex, suffix)| {
                let other_lane = format!("probe.{neighbour}.{}", apex.lanes());
                let beside = match suffix.lane_label() {
                    Some(_) => vec![format!("probe.{apex}"), other_lane],
                    None => vec![other_lane, format!("a.probe.{apex}")],
                };
                beside.into_iter().chain([
                    format!("probe.{}-elsewhere.test", apex.label()),
                    "probe.example.com".to_owned(),
                ])
            })
            .collect();
        CertificateProof::at(now).leaf(root.der(), leaf.der(), &own, &refused)?;
        if let Some(dir) = facts.tls.parent() {
            self.store.state().private_dir(dir)?;
        }
        leaf.write(&facts.tls)?;
        Ok(true)
    }

    /// Writes the instance's edge files, and on a managed machine runs the edge unless it already serves them.
    fn converge_edge(
        &self,
        project: &LocalProject,
        facts: &InstanceFacts,
    ) -> Result<(), LocalError> {
        if project.origins().is_empty() {
            if self.edges {
                self.edge().remove(&facts.key)?;
            }
            return Ok(());
        }
        let caddyfile = Caddyfile::render(facts, project.origins(), self.platform);
        let files = LocalEdge::write(self.store.state(), facts, &caddyfile)?;
        if self.edges {
            self.edge()
                .ensure(self.store.state(), facts, &files, self.platform)?;
        }
        Ok(())
    }

    /// Removes the edge and directory of an instance the registry no longer holds.
    fn give_back(&self, state: &LocalState, key: &InstanceKey) -> Result<(), LocalError> {
        if self.edges {
            self.edge().remove(key)?;
        }
        let dir = state.instance_dir(key);
        match fs::remove_dir_all(&dir) {
            Ok(()) => info!(instance = %key, dir = %dir.display(), "local.instance_given_back"),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(LocalError::io(&dir, "removed", error)),
        }
        Ok(())
    }

    fn converge_resolver(&self, registry: &Registry) -> Result<(), LocalError> {
        let changed = MachineResolver::write(
            self.store.state(),
            &Corefile::render(registry, self.platform),
        )?;
        if self.managed
            && MachineResolver::routed(self.platform)
            && MachineOwner::serves(self.store.state())
        {
            MachineResolver::ensure_running(self.store.state(), self.platform)?;
        }
        info!(
            changed,
            instances = registry.instances().count(),
            "local.converged"
        );
        Ok(())
    }

    /// Whether the resolver's container runs; Docker that cannot be asked counts as not running.
    fn resolver_running(&self) -> bool {
        match MachineResolver::running() {
            Ok(running) => running,
            Err(error) => {
                warn!(code = %error.error_code(), %error, "local.forget: docker cannot be asked, so the resolver cannot be answering");
                false
            }
        }
    }

    /// Whether the resolver stops answering `name` within a reload or two.
    fn resolver_forgets(&self, name: &str) -> bool {
        let server = SocketAddr::new(self.platform.resolver_address().into(), 53);
        let started = Instant::now();
        loop {
            match DnsAnswer::ask(server, name, Duration::from_millis(500)) {
                DnsAnswer::Addresses(_) if started.elapsed() < RESOLVER_SETTLES => {
                    thread::sleep(Duration::from_millis(250));
                }
                DnsAnswer::Addresses(addresses) => {
                    warn!(name, ?addresses, "local.forget: the resolver still answers");
                    return false;
                }
                _ => return true,
            }
        }
    }
}

/// The host label every probe of an instance's names is asked for.
const PROBE_HOST: &str = "fun-local-probe";

/// A lane label that is not this instance's own.
fn neighbour_of(facts: &InstanceFacts) -> &'static str {
    match facts.key.role().label().map(InstanceLabel::as_str) {
        Some("neighbour") => "neighbour-2",
        _ => "neighbour",
    }
}

fn facts_of(
    state: &LocalState,
    key: &InstanceKey,
    project: &LocalProject,
    address: LoopbackAddress,
    root: &ProjectRoot,
) -> InstanceFacts {
    let InstanceNames { key, suffixes } = InstanceNames::of(key.clone(), project);
    InstanceFacts {
        tls: state.instance_dir(&key).join("tls.pem"),
        key,
        address,
        suffixes,
        root: ProjectRoot::certificate_path(state, &root.fingerprint()),
        fingerprint: root.fingerprint(),
    }
}

fn probe_name(facts: &InstanceFacts) -> String {
    facts
        .suffixes
        .first()
        .map(|(_, suffix)| suffix.name(Some(PROBE_HOST)))
        .unwrap_or_default()
}

fn checkout_exists(checkout: &Path) -> bool {
    checkout.join("fun.toml").exists()
}

/// The database node holding `project`'s databases, outside the registry's lock since a first start is slow.
fn converge_database(project: &LocalProject) -> Result<(), LocalError> {
    if project.databases().is_empty() && project.logins().is_empty() {
        return Ok(());
    }
    LocalDatabase.ensure(project.databases(), project.logins())
}
