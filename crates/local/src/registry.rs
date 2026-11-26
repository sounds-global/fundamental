use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use fundamental_core::address::TestApex;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{
    ApexSet, InstanceKey, InstanceLabel, InstanceRole, LocalError, LoopbackAddress, LoopbackPool,
    ProjectName, RootFingerprint,
};

/// This machine's local origins: which project holds which apexes and root, and which checkout
/// holds which instance and loopback address; [`parse`](Self::parse) enforces its uniqueness rules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Registry {
    projects: BTreeMap<ProjectName, ProjectEntry>,
    instances: BTreeMap<InstanceKey, InstanceEntry>,
}

/// One project on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectEntry {
    apexes: ApexSet,
    root: Option<RootFingerprint>,
}

impl ProjectEntry {
    /// The apexes the project's root was minted for.
    #[must_use]
    pub const fn apexes(&self) -> &ApexSet {
        &self.apexes
    }

    /// The project's root, once one has been minted.
    #[must_use]
    pub const fn root(&self) -> Option<&RootFingerprint> {
        self.root.as_ref()
    }
}

/// One instance on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceEntry {
    address: LoopbackAddress,
    checkout: PathBuf,
}

impl InstanceEntry {
    /// The loopback address the instance serves on.
    #[must_use]
    pub const fn address(&self) -> LoopbackAddress {
        self.address
    }

    /// The checkout that holds it.
    #[must_use]
    pub fn checkout(&self) -> &Path {
        &self.checkout
    }
}

/// The registry file as written.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    version: u32,
    #[serde(default, rename = "project")]
    projects: Vec<ProjectRow>,
    #[serde(default, rename = "instance")]
    instances: Vec<InstanceRow>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectRow {
    name: String,
    apexes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    root: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstanceRow {
    project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lane: Option<String>,
    address: String,
    checkout: PathBuf,
}

impl Registry {
    /// The format this `fun` writes, and the newest it reads.
    pub const VERSION: u32 = 1;

    /// Parses a registry file, refusing anything that breaks its rules.
    ///
    /// # Errors
    ///
    /// [`LocalError::RegistryCorrupt`] (`LOCAL_011`) naming every problem.
    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub fn parse(text: &str, path: &Path) -> Result<Self, LocalError> {
        let corrupt = |detail: String| LocalError::RegistryCorrupt {
            path: path.display().to_string(),
            detail,
        };
        let file: RegistryFile =
            toml::from_str(text).map_err(|error| corrupt(error.message().to_owned()))?;
        if file.version > Self::VERSION {
            return Err(corrupt(format!(
                "written by a newer `fun` (format {}; this one reads {})",
                file.version,
                Self::VERSION
            )));
        }
        let mut problems = Vec::new();
        let mut registry = Self::default();
        let mut apex_holders: BTreeMap<TestApex, ProjectName> = BTreeMap::new();
        for row in file.projects {
            let Ok(name) = ProjectName::parse(&row.name) else {
                problems.push(format!("project `{}` is not a DNS label", row.name));
                continue;
            };
            let apexes: Option<Vec<TestApex>> = row
                .apexes
                .iter()
                .map(|apex| TestApex::parse(apex))
                .collect();
            let Some(apexes) = apexes.and_then(ApexSet::new) else {
                problems.push(format!("project `{name}` has an apex that is not one"));
                continue;
            };
            let root = match row.root.as_deref().map(RootFingerprint::parse) {
                Some(None) => {
                    problems.push(format!(
                        "project `{name}` names a root that is no fingerprint"
                    ));
                    continue;
                }
                Some(Some(root)) => Some(root),
                None => None,
            };
            for apex in apexes.iter() {
                if let Some(holder) = apex_holders.insert(apex.clone(), name.clone()) {
                    problems.push(format!("`{apex}` is held by `{holder}` and `{name}`"));
                }
            }
            if registry
                .projects
                .insert(name.clone(), ProjectEntry { apexes, root })
                .is_some()
            {
                problems.push(format!("project `{name}` is listed twice"));
            }
        }
        let mut addresses = BTreeSet::new();
        let mut checkouts = BTreeSet::new();
        for row in file.instances {
            let Ok(project) = ProjectName::parse(&row.project) else {
                problems.push(format!("an instance names project `{}`", row.project));
                continue;
            };
            if !registry.projects.contains_key(&project) {
                problems.push(format!(
                    "an instance names unregistered project `{project}`"
                ));
            }
            let role = match row.lane.as_deref().map(InstanceLabel::parse) {
                None => InstanceRole::Main,
                Some(Ok(label)) => InstanceRole::Lane(label),
                Some(Err(_)) => {
                    problems.push(format!(
                        "an instance of `{project}` has an invalid lane label"
                    ));
                    continue;
                }
            };
            let Some(address) = LoopbackAddress::parse(&row.address) else {
                problems.push(format!("`{}` is not in the loopback pool", row.address));
                continue;
            };
            if !row.checkout.is_absolute() {
                problems.push(format!(
                    "checkout {} is not absolute",
                    row.checkout.display()
                ));
                continue;
            }
            if !addresses.insert(address) {
                problems.push(format!("{address} is held twice"));
            }
            if !checkouts.insert(row.checkout.clone()) {
                problems.push(format!("{} holds two instances", row.checkout.display()));
            }
            let key = InstanceKey::new(project, role);
            if registry
                .instances
                .insert(
                    key.clone(),
                    InstanceEntry {
                        address,
                        checkout: row.checkout,
                    },
                )
                .is_some()
            {
                problems.push(format!("instance {key} is listed twice"));
            }
        }
        if problems.is_empty() {
            Ok(registry)
        } else {
            Err(corrupt(problems.join("; ")))
        }
    }

    /// The registry as the file it is written to.
    ///
    /// # Errors
    ///
    /// [`LocalError::RegistryCorrupt`] if it cannot be serialised, which no valid registry produces.
    pub fn to_toml(&self, path: &Path) -> Result<String, LocalError> {
        let file = RegistryFile {
            version: Self::VERSION,
            projects: self
                .projects
                .iter()
                .map(|(name, entry)| ProjectRow {
                    name: name.to_string(),
                    apexes: entry.apexes.iter().map(ToString::to_string).collect(),
                    root: entry.root.map(|root| root.to_string()),
                })
                .collect(),
            instances: self
                .instances
                .iter()
                .map(|(key, entry)| InstanceRow {
                    project: key.project().to_string(),
                    lane: key.role().label().map(ToString::to_string),
                    address: entry.address.to_string(),
                    checkout: entry.checkout.clone(),
                })
                .collect(),
        };
        toml::to_string(&file).map_err(|error| LocalError::RegistryCorrupt {
            path: path.display().to_string(),
            detail: error.to_string(),
        })
    }

    /// The project registered as `name`.
    #[must_use]
    pub fn project(&self, name: &ProjectName) -> Option<&ProjectEntry> {
        self.projects.get(name)
    }

    /// Every project, by name.
    pub fn projects(&self) -> impl Iterator<Item = (&ProjectName, &ProjectEntry)> {
        self.projects.iter()
    }

    /// Every instance, by key.
    pub fn instances(&self) -> impl Iterator<Item = (&InstanceKey, &InstanceEntry)> {
        self.instances.iter()
    }

    /// The instance `key`.
    #[must_use]
    pub fn instance(&self, key: &InstanceKey) -> Option<&InstanceEntry> {
        self.instances.get(key)
    }

    /// The instance a checkout holds.
    #[must_use]
    pub fn instance_of(&self, checkout: &Path) -> Option<(&InstanceKey, &InstanceEntry)> {
        self.instances
            .iter()
            .find(|(_, entry)| entry.checkout == checkout)
    }

    /// Registers `name` with `apexes` when it is not yet registered, and refuses an apex another
    /// project holds.
    ///
    /// # Errors
    ///
    /// [`LocalError::ApexHeldElsewhere`] (`LOCAL_002`) when another project holds one of `apexes`.
    pub fn register_project(
        &mut self,
        name: &ProjectName,
        apexes: &ApexSet,
    ) -> Result<(), LocalError> {
        self.refuse_held_apexes(name, apexes)?;
        if !self.projects.contains_key(name) {
            info!(project = %name, apexes = %apexes, "local.project_registered");
            self.projects.insert(
                name.clone(),
                ProjectEntry {
                    apexes: apexes.clone(),
                    root: None,
                },
            );
        }
        Ok(())
    }

    /// Records `root` as `name`'s root, minted for `apexes`.
    ///
    /// # Errors
    ///
    /// [`LocalError::ApexHeldElsewhere`] (`LOCAL_002`) when another project holds one of `apexes`.
    pub fn set_root(
        &mut self,
        name: &ProjectName,
        apexes: &ApexSet,
        root: RootFingerprint,
    ) -> Result<(), LocalError> {
        self.refuse_held_apexes(name, apexes)?;
        self.projects.insert(
            name.clone(),
            ProjectEntry {
                apexes: apexes.clone(),
                root: Some(root),
            },
        );
        Ok(())
    }

    fn refuse_held_apexes(&self, name: &ProjectName, apexes: &ApexSet) -> Result<(), LocalError> {
        for (holder, entry) in &self.projects {
            if holder != name
                && let Some(apex) = entry.apexes.overlap(apexes)
            {
                return Err(LocalError::ApexHeldElsewhere {
                    apex: apex.to_string(),
                    holder: holder.to_string(),
                });
            }
        }
        Ok(())
    }

    /// [`claim_instance_in`](Self::claim_instance_in) the machine's pool.
    ///
    /// # Errors
    ///
    /// As [`claim_instance_in`](Self::claim_instance_in).
    pub fn claim_instance(
        &mut self,
        key: &InstanceKey,
        checkout: &Path,
        exists: impl Fn(&Path) -> bool,
    ) -> Result<LoopbackAddress, LocalError> {
        self.claim_instance_in(LoopbackPool::MACHINE, key, checkout, exists)
    }

    /// Claims `key` for `checkout` and answers its address: the one it holds, or the lowest free
    /// one of `pool`; another checkout's instance is taken over only when `exists` says it is gone.
    ///
    /// # Errors
    ///
    /// [`LocalError::InstanceHeldElsewhere`] (`LOCAL_009`) when another existing checkout holds
    /// `key`; [`LocalError::NoAddressLeft`] (`LOCAL_003`) when the pool is spent;
    /// [`LocalError::RegistryCorrupt`] (`LOCAL_011`) when `key`'s project is not registered.
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "readability baseline"
    )]
    pub fn claim_instance_in(
        &mut self,
        pool: LoopbackPool,
        key: &InstanceKey,
        checkout: &Path,
        exists: impl Fn(&Path) -> bool,
    ) -> Result<LoopbackAddress, LocalError> {
        if !self.projects.contains_key(key.project()) {
            return Err(LocalError::RegistryCorrupt {
                path: "the registry in memory".to_owned(),
                detail: format!("an instance of unregistered project `{}`", key.project()),
            });
        }
        if let Some(entry) = self.instances.get_mut(key) {
            if entry.checkout == checkout {
                return Ok(entry.address);
            }
            if exists(&entry.checkout) {
                return Err(LocalError::InstanceHeldElsewhere {
                    instance: key.to_string(),
                    holder: entry.checkout.display().to_string(),
                });
            }
            warn!(
                instance = %key,
                from = %entry.checkout.display(),
                to = %checkout.display(),
                "local.instance_taken_over: the checkout that held it no longer exists"
            );
            entry.checkout = checkout.to_path_buf();
            let address = entry.address;
            self.instances
                .retain(|other, entry| other == key || entry.checkout != checkout);
            return Ok(address);
        }
        if let Some(previous) = self
            .instance_of(checkout)
            .map(|(previous, _)| previous.clone())
        {
            warn!(instance = %previous, checkout = %checkout.display(), "local.instance_released: the checkout now claims another");
            self.instances.remove(&previous);
        }
        let held: BTreeSet<LoopbackAddress> =
            self.instances.values().map(|entry| entry.address).collect();
        let Some(address) = pool.addresses().find(|address| !held.contains(address)) else {
            let orphans: Vec<String> = self
                .instances
                .values()
                .filter(|entry| !exists(&entry.checkout))
                .map(|entry| format!("{} ({})", entry.checkout.display(), entry.address))
                .collect();
            return Err(LocalError::NoAddressLeft {
                orphans: if orphans.is_empty() {
                    "none".to_owned()
                } else {
                    orphans.join(", ")
                },
            });
        };
        info!(instance = %key, %address, checkout = %checkout.display(), "local.address_claimed");
        self.instances.insert(
            key.clone(),
            InstanceEntry {
                address,
                checkout: checkout.to_path_buf(),
            },
        );
        Ok(address)
    }

    /// Gives back the instance `key`, answering what it held.
    pub fn release(&mut self, key: &InstanceKey) -> Option<InstanceEntry> {
        self.instances.remove(key)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use fundamental_core::Error as _;
    use fundamental_core::address::TestApex;

    use super::Registry;
    use crate::{
        ApexSet, InstanceKey, InstanceLabel, InstanceRole, LoopbackAddress, LoopbackPool,
        ProjectName, RootFingerprint,
    };

    fn name(text: &str) -> ProjectName {
        ProjectName::parse(text).expect("a name")
    }

    fn apexes(texts: &[&str]) -> ApexSet {
        ApexSet::new(
            texts
                .iter()
                .map(|text| TestApex::parse(text).expect("an apex")),
        )
        .expect("not empty")
    }

    fn with(projects: &[&str]) -> Registry {
        let mut registry = Registry::default();
        for project in projects {
            registry
                .register_project(&name(project), &apexes(&[&format!("{project}.test")]))
                .expect("free");
        }
        registry
    }

    fn lane(project: &str, label: &str) -> InstanceKey {
        InstanceKey::new(
            name(project),
            InstanceRole::Lane(InstanceLabel::parse(label).expect("a label")),
        )
    }

    #[test]
    fn a_held_apex_is_refused_to_another_project() {
        let mut registry = Registry::default();
        registry
            .register_project(&name("acme"), &apexes(&["acme.test"]))
            .expect("free");
        let refused = registry
            .register_project(&name("other"), &apexes(&["other.test", "acme.test"]))
            .map_err(|error| error.error_code());
        assert_eq!(refused, Err("LOCAL_002"));
        let refused = registry
            .set_root(
                &name("other"),
                &apexes(&["acme.test"]),
                RootFingerprint::of_der(b"x"),
            )
            .map_err(|error| error.error_code());
        assert_eq!(refused, Err("LOCAL_002"));
        assert!(registry.project(&name("other")).is_none());
    }

    #[test]
    fn a_stale_instance_is_taken_over_only_when_its_checkout_is_gone() {
        let mut registry = with(&["acme"]);
        let key = InstanceKey::new(name("acme"), InstanceRole::Main);
        let first = registry
            .claim_instance(&key, Path::new("/a"), |_| true)
            .expect("free");
        assert_eq!(
            registry
                .claim_instance(&key, Path::new("/a"), |_| true)
                .ok(),
            Some(first),
            "a checkout's own claim is idempotent"
        );
        let refused = registry
            .claim_instance(&key, Path::new("/b"), |_| true)
            .map_err(|error| error.error_code());
        assert_eq!(refused, Err("LOCAL_009"));
        let taken = registry
            .claim_instance(&key, Path::new("/b"), |path| path != Path::new("/a"))
            .expect("the holder is gone");
        assert_eq!(taken, first);
        assert_eq!(
            registry
                .instance(&key)
                .map(|entry| entry.checkout().to_path_buf()),
            Some("/b".into())
        );
    }

    #[test]
    fn a_registry_of_the_tests_pool_claims_none_of_the_machines_addresses() {
        let mut registry = with(&["p"]);
        let address = registry
            .claim_instance_in(
                LoopbackPool::TESTS,
                &lane("p", "a"),
                Path::new("/a"),
                |_| true,
            )
            .expect("an address");
        assert_eq!(address.to_string(), "127.0.77.2");
        assert!(!LoopbackPool::MACHINE.holds(address));
    }

    #[test]
    fn every_instance_gets_its_own_address_and_the_pool_is_bounded() {
        let mut registry = with(&["p"]);
        for at in 0..253 {
            let key = lane("p", &format!("l{at}"));
            let address = registry
                .claim_instance(&key, Path::new(&format!("/c{at}")), |_| true)
                .expect("an address");
            assert_eq!(address, LoopbackAddress::new(2 + at).expect("in the pool"));
        }
        let full = registry
            .claim_instance(&lane("p", "late"), Path::new("/late"), |path| {
                path != Path::new("/c7")
            })
            .map_err(|error| (error.error_code(), error.to_string()));
        let (code, message) = full.expect_err("the pool is spent");
        assert_eq!(code, "LOCAL_003");
        assert!(message.contains("/c7 (127.0.0.9)"), "{message}");
        registry.release(&lane("p", "l7"));
        assert_eq!(
            registry
                .claim_instance(&lane("p", "late"), Path::new("/late"), |_| true)
                .ok(),
            LoopbackAddress::new(9)
        );
    }

    #[test]
    fn a_checkout_holds_one_instance() {
        let mut registry = with(&["p"]);
        let unregistered = registry
            .claim_instance(&lane("q", "x"), Path::new("/q"), |_| true)
            .map_err(|error| error.error_code());
        assert_eq!(unregistered, Err("LOCAL_011"));
        let main = InstanceKey::new(name("p"), InstanceRole::Main);
        let address = registry
            .claim_instance(&main, Path::new("/c"), |_| true)
            .expect("free");
        registry
            .claim_instance(&lane("p", "ci"), Path::new("/c"), |_| true)
            .expect("free");
        assert!(registry.instance(&main).is_none());
        assert_eq!(registry.instances().count(), 1);
        assert_eq!(
            registry
                .instance(&lane("p", "ci"))
                .map(|entry| entry.address()),
            Some(address)
        );
    }

    #[test]
    fn the_file_reads_back_what_it_writes() {
        let path = Path::new("/state/registry.toml");
        let mut registry = Registry::default();
        registry
            .set_root(
                &name("acme"),
                &apexes(&["acme.test", "acme-sandbox.test"]),
                RootFingerprint::of_der(b"root"),
            )
            .expect("free");
        registry
            .claim_instance(&lane("acme", "fix-login"), Path::new("/lane"), |_| true)
            .expect("free");
        let text = registry.to_toml(path).expect("serialisable");
        assert_eq!(Registry::parse(&text, path).ok(), Some(registry));
    }

    #[test]
    fn a_file_that_breaks_a_rule_is_refused() {
        let path = Path::new("/state/registry.toml");
        for (text, why) in [
            ("version = 2\n", "newer"),
            (
                "version = 1\n[[project]]\nname = \"a\"\napexes = [\"x.test\"]\n[[project]]\nname = \"b\"\napexes = [\"x.test\"]\n",
                "held by",
            ),
            (
                "version = 1\n[[instance]]\nproject = \"a\"\naddress = \"127.0.0.2\"\ncheckout = \"/c\"\n",
                "unregistered",
            ),
            (
                "version = 1\n[[project]]\nname = \"a\"\napexes = [\"a.test\"]\n[[instance]]\nproject = \"a\"\naddress = \"127.0.0.2\"\ncheckout = \"/c\"\n[[instance]]\nproject = \"a\"\nlane = \"l\"\naddress = \"127.0.0.2\"\ncheckout = \"/d\"\n",
                "held twice",
            ),
            (
                "version = 1\n[[project]]\nname = \"a\"\napexes = [\"a.test\"]\n[[instance]]\nproject = \"a\"\naddress = \"127.0.0.1\"\ncheckout = \"/c\"\n",
                "loopback pool",
            ),
            ("version = 1\nsurprise = true\n", "unknown field"),
        ] {
            let error = Registry::parse(text, path).expect_err(why);
            assert_eq!(error.error_code(), "LOCAL_011");
            assert!(error.to_string().contains(why), "{why}: {error}");
        }
    }
}
