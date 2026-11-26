use fundamental_core::address::TestApex;
use fundamental_manifest::AppKey;

use crate::{InstanceFacts, OriginHost, OriginSurface, OriginVar};

/// One parsed `[local.origins]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalOrigin {
    pub(crate) var: OriginVar,
    pub(crate) app: AppKey,
    pub(crate) surface: OriginSurface,
    pub(crate) port: u16,
    pub(crate) host: OriginHost,
    pub(crate) apex: TestApex,
}

impl LocalOrigin {
    /// The variable the origin reaches every app under.
    #[must_use]
    pub const fn var(&self) -> &OriginVar {
        &self.var
    }

    /// The app it routes to, an app or an instance of one.
    #[must_use]
    pub const fn app(&self) -> &AppKey {
        &self.app
    }

    /// The app's surface it routes to.
    #[must_use]
    pub const fn surface(&self) -> OriginSurface {
        self.surface
    }

    /// The port that surface listens on, from the manifest.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// The apex it is served under.
    #[must_use]
    pub const fn apex(&self) -> &TestApex {
        &self.apex
    }

    /// Where under the instance's suffix it is served.
    #[must_use]
    pub const fn host(&self) -> &OriginHost {
        &self.host
    }

    /// The name this origin has for the instance `facts` describes (`www.acme.test`,
    /// `www.fix-login.acme-lane.test`, `*.preview.acme.test`), or `None` for an apex it does not serve.
    #[must_use]
    pub fn name(&self, facts: &InstanceFacts) -> Option<String> {
        facts
            .suffixes
            .iter()
            .find(|(apex, _)| *apex == self.apex)
            .map(|(_, suffix)| suffix.name(self.host.in_front().as_deref()))
    }

    /// The origin for that instance, `https://www.acme.test`.
    #[must_use]
    pub fn url(&self, facts: &InstanceFacts) -> Option<String> {
        self.name(facts).map(|name| format!("https://{name}"))
    }
}
