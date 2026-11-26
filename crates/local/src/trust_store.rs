use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use rustls_pki_types::CertificateDer;
use tracing::{debug, info, warn};

use crate::tool::{ask, run};
use crate::{
    CertificatePem, LocalError, Platform, Privileged, ProjectName, RootFingerprint, RootProfile,
};

/// One place a browser on this machine reads trusted roots from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustStore {
    /// The system's anchors directory, refreshed by its update tool, both root's.
    System {
        /// The directory anchors are placed in.
        anchors: PathBuf,
        /// The tool that rebuilds the system bundle from them.
        update: PathBuf,
        /// The file extension the update tool reads.
        extension: &'static str,
    },
    /// An NSS database: the user's own (Chrome and Chromium on Linux), or a Firefox profile's.
    Nss(PathBuf),
    /// The macOS System keychain.
    Keychain,
}

/// The candidates for the system anchors on Linux: Fedora and RHEL, then Debian and Ubuntu.
const LINUX_SYSTEMS: &[(&str, &[&str], &str)] = &[
    (
        "/etc/pki/ca-trust/source/anchors",
        &["/usr/bin/update-ca-trust", "/bin/update-ca-trust"],
        "pem",
    ),
    (
        "/usr/local/share/ca-certificates",
        &[
            "/usr/sbin/update-ca-certificates",
            "/sbin/update-ca-certificates",
        ],
        "crt",
    ),
];

/// Where `certutil` is: run as the developer, it only touches the developer's own databases.
const CERTUTIL: &[&str] = &[
    "/usr/bin/certutil",
    "/usr/local/bin/certutil",
    "/opt/homebrew/bin/certutil",
];
const KEYCHAIN: &str = "/Library/Keychains/System.keychain";
/// How many earlier roots under one nickname `install` removes before adding the new one.
const EARLIER_ROOTS: usize = 8;
const SECURITY: &str = "/usr/bin/security";

impl TrustStore {
    /// Every store a browser on this machine reads.
    ///
    /// # Errors
    ///
    /// [`LocalError::UnsupportedPlatform`] (`LOCAL_013`) on Linux with no known anchors directory.
    pub fn every(platform: Platform) -> Result<Vec<Self>, LocalError> {
        let mut stores = Vec::new();
        match platform {
            Platform::MacOs => stores.push(Self::Keychain),
            Platform::Linux => {
                let system = LINUX_SYSTEMS
                    .iter()
                    .find_map(|(anchors, update, extension)| {
                        let anchors = Path::new(anchors);
                        let update = Privileged::tool(update).ok()?;
                        anchors.is_dir().then(|| Self::System {
                            anchors: anchors.to_path_buf(),
                            update,
                            extension,
                        })
                    })
                    .ok_or_else(|| LocalError::UnsupportedPlatform {
                        detail: "no system trust anchors directory with its update tool was found"
                            .to_owned(),
                    })?;
                stores.push(system);
            }
        }
        if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
            if platform == Platform::Linux {
                stores.push(Self::Nss(home.join(".pki").join("nssdb")));
            }
            let profiles = match platform {
                Platform::Linux => home.join(".mozilla").join("firefox"),
                Platform::MacOs => home.join("Library/Application Support/Firefox/Profiles"),
            };
            if let Ok(entries) = fs::read_dir(&profiles) {
                let mut found: Vec<PathBuf> = entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| path.join("cert9.db").is_file())
                    .collect();
                found.sort();
                stores.extend(found.into_iter().map(Self::Nss));
            }
        }
        Ok(stores)
    }

    /// What this store is called in a report.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::System { anchors, .. } => format!("the system anchors ({})", anchors.display()),
            Self::Nss(dir) => format!("the NSS database {}", dir.display()),
            Self::Keychain => "the System keychain".to_owned(),
        }
    }

    /// The anchor file of `project` in the system store.
    fn anchor(anchors: &Path, project: &ProjectName, extension: &str) -> PathBuf {
        anchors.join(format!("fundamental-local-{project}.{extension}"))
    }

    /// The nickname `project`'s root carries in an NSS database or keychain.
    fn nickname(project: &ProjectName) -> String {
        format!("fundamental local {project}")
    }

    /// Refuses `apexes` if another project's root this store trusts permits one of them.
    ///
    /// # Errors
    ///
    /// [`LocalError::ApexHeldElsewhere`] (`LOCAL_002`) naming the installed anchor.
    pub fn refuse_overlap(
        &self,
        project: &ProjectName,
        apexes: &[String],
    ) -> Result<(), LocalError> {
        let Self::System {
            anchors, extension, ..
        } = self
        else {
            return Ok(());
        };
        let ours = Self::anchor(anchors, project, extension);
        let entries =
            fs::read_dir(anchors).map_err(|error| LocalError::io(anchors, "read", error))?;
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let named = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("fundamental-local-"));
            if !named || path == ours {
                continue;
            }
            refuse_overlap_with(&path, apexes)?;
        }
        Ok(())
    }

    /// Installs `der`, already judged, as `project`'s root, encoded as exactly one PEM block, then reads it back.
    ///
    /// # Errors
    ///
    /// [`LocalError::Io`] when the anchors directory cannot be read;
    /// [`LocalError::ToolFailed`] (`LOCAL_012`) when a step fails;
    /// [`LocalError::RootNotTrusted`] (`LOCAL_005`) when the read-back differs.
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub fn install(
        &self,
        project: &ProjectName,
        der: &CertificateDer<'_>,
    ) -> Result<(), LocalError> {
        let pem = CertificatePem::encode(der);
        let pem = pem.as_str();
        match self {
            Self::System {
                anchors,
                update,
                extension,
            } => {
                let anchor = Self::anchor(anchors, project, extension);
                Privileged::write(&anchor, pem)?;
                let args: &[&str] = if update.ends_with("update-ca-trust") {
                    &["extract"]
                } else {
                    &[]
                };
                Privileged::run(update, args, None)?;
            }
            Self::Nss(dir) => {
                let certutil = certutil()?;
                let database = format!("sql:{}", dir.display());
                if !dir.join("cert9.db").is_file() {
                    fs::create_dir_all(dir)
                        .map_err(|error| LocalError::io(dir, "created", error))?;
                    run(
                        &certutil,
                        &["-N", "--empty-password", "-d", &database],
                        None,
                    )?;
                }
                let nickname = Self::nickname(project);
                for _ in 0..EARLIER_ROOTS {
                    if ask(&certutil, &["-D", "-d", &database, "-n", &nickname]).is_err() {
                        break;
                    }
                    info!(database = %database, %nickname, "an earlier root under this nickname was removed");
                }
                run(
                    &certutil,
                    &["-A", "-a", "-d", &database, "-t", "C,,", "-n", &nickname],
                    Some(pem.as_bytes()),
                )?;
            }
            Self::Keychain => {
                let staged = format!("/etc/fundamental/roots/{project}.pem");
                let mkdir = Privileged::tool(&["/bin/mkdir"])?;
                Privileged::run(&mkdir, &["-p", "/etc/fundamental/roots"], None)?;
                Privileged::write(Path::new(&staged), pem)?;
                let security = Privileged::tool(&[SECURITY])?;
                Privileged::run(
                    &security,
                    &[
                        "add-trusted-cert",
                        "-d",
                        "-r",
                        "trustRoot",
                        "-k",
                        KEYCHAIN,
                        &staged,
                    ],
                    None,
                )?;
            }
        }
        if self.holds(project, der) {
            info!(store = %self.describe(), fingerprint = %RootFingerprint::of_der(der), "local.trusted");
            Ok(())
        } else {
            Err(LocalError::RootNotTrusted {
                project: project.to_string(),
                stores: format!("{} (read back after installing)", self.describe()),
            })
        }
    }

    /// Whether this store holds exactly `der` as `project`'s root, and nothing beside it.
    #[must_use]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn holds(&self, project: &ProjectName, der: &CertificateDer<'_>) -> bool {
        let installed = match self {
            Self::System {
                anchors, extension, ..
            } => {
                let anchor = Self::anchor(anchors, project, extension);
                return match fs::read_to_string(&anchor) {
                    Ok(text) if text == CertificatePem::encode(der) => true,
                    Ok(_) => {
                        warn!(anchor = %anchor.display(), "the installed anchor is not exactly this root");
                        false
                    }
                    Err(error) => {
                        debug!(anchor = %anchor.display(), %error, "no anchor installed");
                        false
                    }
                };
            }
            Self::Nss(dir) => certutil().ok().and_then(|certutil| {
                let database = format!("sql:{}", dir.display());
                ask(
                    &certutil,
                    &["-L", "-d", &database, "-n", &Self::nickname(project), "-a"],
                )
                .ok()
            }),
            Self::Keychain => ask(
                SECURITY,
                &[
                    "find-certificate",
                    "-a",
                    "-p",
                    "-c",
                    &Self::nickname(project),
                    KEYCHAIN,
                ],
            )
            .ok(),
        };
        let Some(installed) = installed else {
            debug!(store = %self.describe(), "no root installed under this project's nickname");
            return false;
        };
        match CertificatePem::single(&installed) {
            Ok(found) if found == *der => true,
            Ok(_) => {
                warn!(store = %self.describe(), "the installed root is another certificate");
                false
            }
            Err(detail) => {
                warn!(store = %self.describe(), %detail, "the installed root is not exactly one certificate");
                false
            }
        }
    }
}

fn certutil() -> Result<String, LocalError> {
    CERTUTIL
        .iter()
        .find(|path| Path::new(path).is_file())
        .map(|path| (*path).to_owned())
        .ok_or_else(|| LocalError::ToolFailed {
            argv: "certutil".to_owned(),
            status: "not found".to_owned(),
            stderr: "install NSS's tools (Fedora: nss-tools, Debian: libnss3-tools) so Chrome and Firefox trust the root".to_owned(),
        })
}

/// Refuses `apexes` when the anchor at `path` permits one of them.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn refuse_overlap_with(path: &Path, apexes: &[String]) -> Result<(), LocalError> {
    let text = fs::read_to_string(path).map_err(|error| LocalError::io(path, "read", error))?;
    let der = match CertificatePem::single(&text) {
        Ok(der) => der,
        Err(detail) => {
            warn!(anchor = %path.display(), %detail, "an installed local anchor is not one certificate; it is not compared");
            return Ok(());
        }
    };
    let permitted = match RootProfile::exact(&der) {
        Ok(profile) => profile.permitted().to_vec(),
        Err(error) => {
            warn!(anchor = %path.display(), %error, "an installed local anchor is not of the minted profile; it is not compared");
            return Ok(());
        }
    };
    match apexes.iter().find(|apex| permitted.contains(apex)) {
        Some(apex) => Err(LocalError::ApexHeldElsewhere {
            apex: apex.clone(),
            holder: format!("the root trusted at {}", path.display()),
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fundamental_core::Error as _;
    use fundamental_core::address::TestApex;
    use time::OffsetDateTime;

    use super::TrustStore;
    use crate::{ApexSet, CertificatePem, ProjectName, ProjectRoot};

    #[test]
    fn trust_refuses_an_overlapping_root() {
        let dir = tempfile::tempdir().expect("an anchors directory");
        let theirs = ProjectRoot::mint(
            &ProjectName::parse("other").expect("a name"),
            &ApexSet::new(TestApex::parse("shop.test")).expect("one"),
            OffsetDateTime::now_utc(),
        )
        .expect("a root");
        std::fs::write(dir.path().join("fundamental-local-other.pem"), theirs.pem())
            .expect("installed");
        let store = TrustStore::System {
            anchors: dir.path().to_path_buf(),
            update: PathBuf::from("/usr/bin/true"),
            extension: "pem",
        };
        let project = ProjectName::parse("shop").expect("a name");
        let refused = store
            .refuse_overlap(&project, &["shop.test".to_owned()])
            .map_err(|error| (error.error_code(), error.to_string()));
        let (code, message) = refused.expect_err("another project's root permits it");
        assert_eq!(code, "LOCAL_002");
        assert!(message.contains("fundamental-local-other.pem"), "{message}");
        assert!(
            store
                .refuse_overlap(&project, &["days.test".to_owned()])
                .is_ok()
        );
        std::fs::rename(
            dir.path().join("fundamental-local-other.pem"),
            dir.path().join("fundamental-local-shop.pem"),
        )
        .expect("renamed");
        assert!(
            store
                .refuse_overlap(&project, &["shop.test".to_owned()])
                .is_ok(),
            "a project's own earlier anchor is replaced, not refused"
        );
    }

    #[test]
    fn a_system_anchor_holding_more_than_the_root_is_not_trusted() {
        let dir = tempfile::tempdir().expect("an anchors directory");
        let project = ProjectName::parse("acme").expect("a name");
        let mint = |name: &str, apex: &str| {
            ProjectRoot::mint(
                &ProjectName::parse(name).expect("a name"),
                &ApexSet::new(TestApex::parse(apex)).expect("one"),
                OffsetDateTime::now_utc(),
            )
            .expect("a root")
        };
        let ours = mint("acme", "acme.test");
        let store = TrustStore::System {
            anchors: dir.path().to_path_buf(),
            update: PathBuf::from("/usr/bin/true"),
            extension: "pem",
        };
        let anchor = dir.path().join("fundamental-local-acme.pem");
        assert!(!store.holds(&project, ours.der()), "nothing installed");
        std::fs::write(&anchor, CertificatePem::encode(ours.der())).expect("installed");
        assert!(store.holds(&project, ours.der()));
        let planted = format!(
            "{}{}",
            CertificatePem::encode(ours.der()),
            mint("evil", "evil.test").pem()
        );
        std::fs::write(&anchor, planted).expect("a second block planted");
        assert!(
            !store.holds(&project, ours.der()),
            "an anchor with a second block is refused"
        );
    }
}
