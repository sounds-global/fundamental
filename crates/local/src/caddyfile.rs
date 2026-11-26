use crate::{InstanceFacts, LocalOrigins, Platform};

/// One instance's edge configuration: Caddy serving each declared origin on the instance's
/// address and port 443, proxying to the app's declared port.
///
/// - No admin API, no ACME, no `:80`.
/// - Every forwarded header a client sends is removed and none added: any local process can
///   send from any loopback address, so an address proves nothing.
/// - Only `GET`/`HEAD` under `/_fundamental/` is compressed, as in production.
/// - Any other host is `421`, and an SNI that disagrees with `Host` is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caddyfile(String);

/// Where the edge's container sees the instance's directory, holding `tls.pem` and this file.
pub(crate) const EDGE_DIR: &str = "/etc/fundamental";

/// The headers a client may send to claim where a request came from or what it was for.
const FORWARDED: &[&str] = &[
    "Forwarded",
    "X-Forwarded-For",
    "X-Forwarded-Host",
    "X-Forwarded-Proto",
    "X-Forwarded-Port",
    "X-Real-Ip",
];

impl Caddyfile {
    /// Renders the edge of the instance `facts` describes, serving `origins`, for `platform`.
    #[must_use]
    pub fn render(facts: &InstanceFacts, origins: &LocalOrigins, platform: Platform) -> Self {
        let address = facts.address;
        let bind = platform.edge_bind(address);
        let mut out = format!(
            "# Written by `fun local up` for {key}; recreated, never reloaded.\n{{\n    admin off\n    auto_https off\n    persist_config off\n    default_bind {bind}\n    servers {{\n        strict_sni_host on\n    }}\n}}\n\n(fundamental_edge) {{\n    tls {EDGE_DIR}/tls.pem {EDGE_DIR}/tls.pem\n",
            key = facts.key,
        );
        for header in FORWARDED {
            out.push_str(&format!("    request_header -{header}\n"));
        }
        out.push_str("}\n");
        for origin in origins.iter() {
            let Some(name) = origin.name(facts) else {
                continue;
            };
            let upstream = match platform {
                Platform::Linux => format!("{address}:{}", origin.port()),
                Platform::MacOs => format!("host.docker.internal:{}", origin.port()),
            };
            out.push_str(&format!(
                "\n# {var}: {app} {surface}\nhttps://{name}:443 {{\n    import fundamental_edge\n    reverse_proxy {upstream} {{\n",
                var = origin.var(),
                app = origin.app(),
                surface = origin.surface(),
            ));
            for header in FORWARDED
                .iter()
                .filter(|header| header.starts_with("X-Forwarded"))
            {
                out.push_str(&format!("        header_up -{header}\n"));
            }
            out.push_str("    }\n    @fundamental_bundle {\n        method GET HEAD\n        path /_fundamental/*\n    }\n    encode @fundamental_bundle zstd gzip\n}\n");
        }
        out.push_str("\nhttps://:443 {\n    import fundamental_edge\n    respond 421\n}\n");
        Self(out)
    }

    /// The configuration's text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use fundamental_manifest::Manifest;

    use super::Caddyfile;
    use crate::{
        InstanceFacts, InstanceKey, InstanceLabel, InstanceRole, LocalProject, LoopbackAddress,
        Platform, RootFingerprint,
    };

    #[test]
    fn a_wildcard_origin_is_one_site_for_every_name_under_its_label() {
        let dir = tempfile::tempdir().expect("a directory");
        fs::write(
            dir.path().join("fun.toml"),
            "[project]\nname = \"acme\"\n\n[apps.www.web]\nport = 3000\n\n[local]\napexes = [\"acme.test\"]\n\n[local.origins]\nPREVIEW_ORIGIN = { app = \"www\", host = \"*.preview\", apex = \"acme.test\" }\n",
        )
        .expect("a manifest");
        let project = LocalProject::from_manifest(&Manifest::discover(dir.path()).expect("found"))
            .expect("a project");
        let key = InstanceKey::new(project.name().clone(), InstanceRole::Main);
        let facts = InstanceFacts {
            suffixes: project
                .apexes()
                .iter()
                .map(|apex| (apex.clone(), key.role().suffix(apex)))
                .collect(),
            key,
            address: LoopbackAddress::new(9).expect("an address"),
            tls: "/s/tls.pem".into(),
            root: "/s/root.pem".into(),
            fingerprint: RootFingerprint::of_der(&[]),
        };

        let text = Caddyfile::render(&facts, project.origins(), Platform::Linux);

        assert!(
            text.as_str().contains("https://*.preview.acme.test:443 {\n    import fundamental_edge\n    reverse_proxy 127.0.0.9:3000 {"),
            "{}",
            text.as_str()
        );
    }

    #[test]
    fn the_edge_serves_each_origin_at_the_instances_names_and_nothing_else() {
        let dir = tempfile::tempdir().expect("a directory");
        fs::write(
            dir.path().join("fun.toml"),
            "[project]\nname = \"acme\"\n\n[apps.www.web]\nport = 3000\n\n[local]\napexes = [\"acme.test\"]\n\n[local.origins]\nWWW_ORIGIN = { app = \"www\", host = \"www\", apex = \"acme.test\" }\n",
        )
        .expect("a manifest");
        let project = LocalProject::from_manifest(&Manifest::discover(dir.path()).expect("found"))
            .expect("a project");
        let key = InstanceKey::new(
            project.name().clone(),
            InstanceRole::Lane(InstanceLabel::parse("fix-login").expect("a label")),
        );
        let facts = InstanceFacts {
            suffixes: project
                .apexes()
                .iter()
                .map(|apex| (apex.clone(), key.role().suffix(apex)))
                .collect(),
            key,
            address: LoopbackAddress::new(9).expect("an address"),
            tls: "/s/tls.pem".into(),
            root: "/s/root.pem".into(),
            fingerprint: RootFingerprint::of_der(&[]),
        };
        let linux = Caddyfile::render(&facts, project.origins(), Platform::Linux);
        let text = linux.as_str();
        for expected in [
            "admin off",
            "auto_https off",
            "default_bind 127.0.0.9",
            "strict_sni_host on",
            "https://www.fix-login.acme-lane.test:443 {",
            "reverse_proxy 127.0.0.9:3000 {",
            "request_header -X-Forwarded-For",
            "request_header -Forwarded",
            "request_header -X-Real-Ip",
            "header_up -X-Forwarded-For",
            "respond 421",
            "method GET HEAD\n        path /_fundamental/*",
            "encode @fundamental_bundle zstd gzip",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in\n{text}");
        }
        assert!(!text.contains(":80"), "{text}");
        assert_eq!(
            text.matches("encode ").count(),
            1,
            "only the bundle is compressed, never an answer a session reads: {text}"
        );
        assert!(
            !text.contains("https://www.acme.test"),
            "a lane never serves main's name"
        );
        let mac = Caddyfile::render(&facts, project.origins(), Platform::MacOs);
        for expected in [
            "reverse_proxy host.docker.internal:3000",
            "default_bind 0.0.0.0",
        ] {
            assert!(mac.as_str().contains(expected), "{}", mac.as_str());
        }
    }
}
