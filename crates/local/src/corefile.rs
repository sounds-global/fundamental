use std::fmt::Write as _;

use crate::{InstanceRole, Platform, Registry};

/// The machine resolver's `Corefile`, rendered from the registry alone.
///
/// Each apex is two zones, main's and its lanes twin `<apex>-lane.test`, so a forgotten lane's
/// names can never fall through to main. Outside every zone it answers `REFUSED`. Every value
/// comes from a parsed type, so nothing can carry a brace or newline into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Corefile(String);

impl Corefile {
    /// Renders the resolver's configuration for `registry` on `platform`.
    #[must_use]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub fn render(registry: &Registry, platform: Platform) -> Self {
        let mut out = String::from(
            "# Written by `fun local` from its registry; `fun local up` and `forget` rewrite it.\n",
        );
        let header = server_header(platform);
        for (project, entry) in registry.projects() {
            for apex in entry.apexes().iter() {
                let lanes = apex.lanes();
                for (zone, main) in [(apex.as_str(), true), (lanes.as_str(), false)] {
                    let _ = writeln!(out, "{zone}:53 {{");
                    out.push_str(&header);
                    let pattern_zone = escape(zone);
                    for (key, instance) in registry.instances() {
                        let pattern = match key.role() {
                            _ if key.project() != project => continue,
                            InstanceRole::Main if main => {
                                format!("^([a-z0-9-]+[.]){{0,2}}{pattern_zone}[.]$")
                            }
                            InstanceRole::Lane(label) if !main => format!(
                                "^([a-z0-9-]+[.]){{0,2}}{}[.]{pattern_zone}[.]$",
                                escape(label.as_str())
                            ),
                            InstanceRole::Main | InstanceRole::Lane(_) => continue,
                        };
                        let address = instance.address();
                        let _ = write!(
                            out,
                            "    template IN A {zone} {{\n        match \"{pattern}\"\n        answer \"{{{{ .Name }}}} 60 IN A {address}\"\n        fallthrough\n    }}\n    template IN AAAA {zone} {{\n        match \"{pattern}\"\n        rcode NOERROR\n        fallthrough\n    }}\n"
                        );
                    }
                    let _ = write!(
                        out,
                        "    template ANY ANY {zone} {{\n        rcode NXDOMAIN\n    }}\n}}\n"
                    );
                }
            }
        }
        let _ = write!(
            out,
            ".:53 {{\n{header}    template ANY ANY . {{\n        rcode REFUSED\n    }}\n}}\n"
        );
        Self(out)
    }

    /// The configuration's text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What every server block starts with: where it listens, who may ask, and how it reloads.
///
/// On Linux only queries from the routing address or loopback are answered (a LAN host can
/// reach `192.0.2.53` under the weak host model); on macOS the `127.0.0.1` publish is the boundary.
fn server_header(platform: Platform) -> String {
    let mut header = String::new();
    if platform == Platform::Linux {
        let _ = write!(
            header,
            "    bind {}\n    acl {{\n        allow net {}/32 127.0.0.0/8\n        drop\n    }}\n",
            Platform::ROUTING_ADDRESS,
            Platform::ROUTING_ADDRESS
        );
    }
    header.push_str("    errors\n    reload 2s 1s\n");
    header
}

/// `text` with every `.` matched literally, as a regular expression.
fn escape(text: &str) -> String {
    text.replace('.', "[.]")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use fundamental_core::address::TestApex;

    use super::Corefile;
    use crate::{
        ApexSet, InstanceKey, InstanceLabel, InstanceRole, Platform, ProjectName, Registry,
    };

    #[test]
    fn every_instance_answers_under_its_own_suffix_and_nothing_else_does() {
        let mut registry = Registry::default();
        let project = ProjectName::parse("acme").expect("a name");
        let apexes = ApexSet::new(TestApex::parse("acme.test")).expect("one");
        registry.register_project(&project, &apexes).expect("free");
        registry
            .claim_instance(
                &InstanceKey::new(project.clone(), InstanceRole::Main),
                Path::new("/m"),
                |_| true,
            )
            .expect("an address");
        let lane = InstanceRole::Lane(InstanceLabel::parse("fix-login").expect("a label"));
        registry
            .claim_instance(&InstanceKey::new(project, lane), Path::new("/l"), |_| true)
            .expect("an address");
        let linux = Corefile::render(&registry, Platform::Linux);
        let text = linux.as_str();
        assert!(text.contains("acme.test:53 {"), "{text}");
        assert!(text.contains("acme-lane.test:53 {"), "{text}");
        assert!(
            text.contains("match \"^([a-z0-9-]+[.]){0,2}acme[.]test[.]$\""),
            "{text}"
        );
        assert!(
            text.contains("match \"^([a-z0-9-]+[.]){0,2}fix-login[.]acme-lane[.]test[.]$\""),
            "{text}"
        );
        let (main_zone, lanes_zone) = text
            .split_once("acme-lane.test:53 {")
            .expect("the lanes zone follows the apex");
        assert!(
            !main_zone.contains("fix-login"),
            "a lane answers in main's zone: {text}"
        );
        assert!(
            !lanes_zone.contains("acme[.]test"),
            "main answers in the lanes zone: {text}"
        );
        assert!(text.contains("60 IN A 127.0.0.2"), "{text}");
        assert!(text.contains("60 IN A 127.0.0.3"), "{text}");
        assert!(text.contains("rcode NXDOMAIN"), "{text}");
        assert!(text.contains("rcode REFUSED"), "{text}");
        assert!(text.contains("bind 192.0.2.53"), "{text}");
        assert!(
            text.contains("allow net 192.0.2.53/32 127.0.0.0/8"),
            "{text}"
        );
        let mac = Corefile::render(&registry, Platform::MacOs);
        assert!(!mac.as_str().contains("bind "), "{}", mac.as_str());
        assert!(!mac.as_str().contains("acl"), "{}", mac.as_str());
    }
}
