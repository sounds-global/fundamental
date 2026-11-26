use std::net::Ipv4Addr;
use std::path::PathBuf;

use fundamental_core::address::{LocalSuffix, TestApex};

use crate::instance_names::shell_lines;
use crate::{InstanceKey, InstanceNames, LoopbackAddress, RootFingerprint};

/// Everything one instance is on this machine: what `fun local env` prints and `fun dev` gives its apps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceFacts {
    /// The project and which of its checkouts.
    pub key: InstanceKey,
    /// The loopback address the instance serves on.
    pub address: LoopbackAddress,
    /// Each apex, with the suffix this instance's names end in under it.
    pub suffixes: Vec<(TestApex, LocalSuffix)>,
    /// The instance's certificate and key, `tls.pem`.
    pub tls: PathBuf,
    /// The project's root certificate, `root.pem`.
    pub root: PathBuf,
    /// The root's fingerprint.
    pub fingerprint: RootFingerprint,
}

impl InstanceFacts {
    /// Who the instance is and the names it serves, apart from what it has claimed.
    #[must_use]
    pub fn names(&self) -> InstanceNames {
        InstanceNames {
            key: self.key.clone(),
            suffixes: self.suffixes.clone(),
        }
    }

    /// The facts as environment variables, in a fixed order.
    #[must_use]
    pub fn variables(&self) -> Vec<(String, String)> {
        let mut variables = self.names().variables();
        variables.extend([
            ("FUN_LOCAL_ADDRESS".to_owned(), self.address.to_string()),
            ("FUN_LOCAL_TLS".to_owned(), self.tls.display().to_string()),
            ("FUN_LOCAL_ROOT".to_owned(), self.root.display().to_string()),
            (
                "FUN_LOCAL_ROOT_FINGERPRINT".to_owned(),
                self.fingerprint.to_string(),
            ),
        ]);
        variables
    }

    /// The facts as `NAME='value'` lines a POSIX shell can `eval`.
    #[must_use]
    pub fn shell(&self) -> String {
        shell_lines(&self.variables())
    }

    /// The address as an IP.
    #[must_use]
    pub const fn ip(&self) -> Ipv4Addr {
        self.address.ip()
    }

    /// The suffix under `apex`, if the project serves under it.
    #[must_use]
    pub fn suffix(&self, apex: &TestApex) -> Option<&LocalSuffix> {
        self.suffixes
            .iter()
            .find(|(candidate, _)| candidate == apex)
            .map(|(_, suffix)| suffix)
    }
}
