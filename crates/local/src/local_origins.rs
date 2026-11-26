use std::collections::{BTreeMap, BTreeSet};

use fundamental_core::address::TestApex;
use fundamental_manifest::{APP_LISTENERS, AppKey, KeyPath, Manifest};
use serde::Deserialize;

use crate::{
    ApexSource, InstanceFacts, LocalError, LocalOrigin, OriginHost, OriginSurface, OriginVar,
    Platform,
};

/// Every origin a project's local edge serves, parsed from `[local.origins]`:
///
/// ```toml
/// [local.origins]
/// WWW_ORIGIN = { app = "www", host = "www" }               # a brand project: the app's brand
/// API_ORIGIN = { app = "api", host = "api", surface = "api", apex = "acme.test" }
/// ```
///
/// With brands an origin's apex is its app's brand; without, one of the declared apexes. Every
/// other listener of a routed app moves onto the instance's address too.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalOrigins {
    origins: Vec<LocalOrigin>,
    others: BTreeMap<AppKey, BTreeSet<&'static str>>,
}

/// One entry as written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OriginRow {
    app: String,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    surface: Option<String>,
    #[serde(default)]
    apex: Option<String>,
}

/// The value configuration reads as "TLS ends before this listener" (`TlsPosture`).
const TERMINATED_UPSTREAM: &str = "terminated_upstream";

impl LocalOrigins {
    /// Parses `table`, judging each entry against `manifest`'s apps and `apexes`.
    ///
    /// # Errors
    ///
    /// [`LocalError::OriginsInvalid`] (`LOCAL_010`) listing every problem at once.
    pub fn parse(
        table: &toml::Table,
        manifest: &Manifest,
        apexes: &ApexSource,
    ) -> Result<Self, LocalError> {
        let mut problems = Vec::new();
        let mut origins = Vec::new();
        for (var, value) in table {
            match Self::entry(var, value, manifest, apexes) {
                Ok(origin) => origins.push(origin),
                Err(mut found) => problems.append(&mut found),
            }
        }
        let mut names = BTreeSet::new();
        for origin in &origins {
            if !names.insert((origin.host.clone(), origin.apex.clone())) {
                problems.push(format!(
                    "`{}`: another origin is already served at {} under {}",
                    origin.var,
                    origin.host(),
                    origin.apex
                ));
            }
        }
        if problems.is_empty() {
            let others = Self::others(&origins, manifest);
            Ok(Self { origins, others })
        } else {
            Err(LocalError::OriginsInvalid {
                problems: problems.join("\n"),
            })
        }
    }

    /// Each routed app's listeners the edge does not route to.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn others(
        origins: &[LocalOrigin],
        manifest: &Manifest,
    ) -> BTreeMap<AppKey, BTreeSet<&'static str>> {
        let mut others = BTreeMap::new();
        for origin in origins {
            let app = origin.app.as_str();
            let base = manifest
                .get(&KeyPath::from_segments(["apps", app, "instance_of"]))
                .and_then(toml::Value::as_str);
            let declared = |listener: &str| {
                [Some(app), base].into_iter().flatten().any(|owner| {
                    manifest
                        .get(&KeyPath::from_segments(["apps", owner, listener, "port"]))
                        .is_some()
                })
            };
            let routed: BTreeSet<&str> = origins
                .iter()
                .filter(|routed| routed.app == origin.app)
                .map(|routed| routed.surface.key())
                .collect();
            let listeners: BTreeSet<&'static str> = APP_LISTENERS
                .into_iter()
                .filter(|listener| !routed.contains(listener) && declared(listener))
                .collect();
            others.insert(origin.app.clone(), listeners);
        }
        others
    }

    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    fn entry(
        var: &str,
        value: &toml::Value,
        manifest: &Manifest,
        apexes: &ApexSource,
    ) -> Result<LocalOrigin, Vec<String>> {
        let mut problems = Vec::new();
        let row = match value.clone().try_into::<OriginRow>() {
            Ok(row) => row,
            Err(error) => {
                return Err(vec![format!(
                    "`{var}` is not `{{ app, host, surface, apex }}`: {}",
                    error.message()
                )]);
            }
        };
        let parsed_var = OriginVar::parse(var).map_err(|why| problems.push(why)).ok();
        let surface = OriginSurface::parse(row.surface.as_deref().unwrap_or("web"))
            .map_err(|why| problems.push(format!("`{var}`: {why}")))
            .ok();
        let Some(label) = AppKey::parse(&row.app) else {
            problems.push(format!(
                "`{var}`: app `{}` is not an app key (a lower-case letter, then letters and digits with single inner `_` or `-`), and the edge names it",
                row.app.escape_debug()
            ));
            return Err(problems);
        };
        let app = KeyPath::from_segments(["apps", label.as_str()]);
        let port = if manifest.get(&app).is_none() {
            problems.push(format!("`{var}`: there is no `[apps.{}]`", row.app));
            None
        } else {
            surface.and_then(|surface| {
                let key = KeyPath::from_segments(["apps", row.app.as_str(), surface.key(), "port"]);
                match manifest.get(&key).and_then(toml::Value::as_integer).map(u16::try_from) {
                    Some(Ok(port)) if port > 0 => Some(port),
                    _ => {
                        problems.push(format!(
                            "`{var}`: `[apps.{}.{surface}] port` is not declared as a port, and the edge routes to it",
                            row.app
                        ));
                        None
                    }
                }
            })
        };
        let host = OriginHost::parse(row.host.as_deref());
        if host.is_none() {
            problems.push(format!(
                "`{var}`: host `{}` is not one lower-case DNS label, nor `*.` and one",
                row.host.as_deref().unwrap_or_default()
            ));
        }
        let apex = Self::apex_of(var, &row, manifest, apexes)
            .map_err(|why| problems.push(why))
            .ok();
        match (parsed_var, surface, port, host, apex) {
            (Some(var), Some(surface), Some(port), Some(host), Some(apex))
                if problems.is_empty() =>
            {
                Ok(LocalOrigin {
                    var,
                    app: label,
                    surface,
                    port,
                    host,
                    apex,
                })
            }
            _ => Err(problems),
        }
    }

    fn apex_of(
        var: &str,
        row: &OriginRow,
        manifest: &Manifest,
        apexes: &ApexSource,
    ) -> Result<TestApex, String> {
        match apexes {
            ApexSource::Brands { by_brand, .. } => {
                if row.apex.is_some() {
                    return Err(format!(
                        "`{var}`: `apex` is decided by `[apps.{}] brand` in a project with brands",
                        row.app
                    ));
                }
                let key = KeyPath::from_segments(["apps", row.app.as_str(), "brand"]);
                let Some(code) = manifest.get(&key).and_then(toml::Value::as_str) else {
                    return Err(format!(
                        "`{var}`: `[apps.{}] brand` names no brand, so the origin has no apex",
                        row.app
                    ));
                };
                by_brand.get(code).cloned().ok_or_else(|| {
                    format!(
                        "`{var}`: `[apps.{}] brand = \"{code}\"` is not a brand in `brands/`",
                        row.app
                    )
                })
            }
            ApexSource::Declared(declared) => {
                let Some(written) = row.apex.as_deref() else {
                    return Err(format!(
                        "`{var}`: `apex` must name one of [local] apexes ({declared})"
                    ));
                };
                TestApex::parse(written)
                    .filter(|apex| declared.contains(apex))
                    .ok_or_else(|| {
                        format!(
                            "`{var}`: apex `{written}` is not one of [local] apexes ({declared})"
                        )
                    })
            }
        }
    }

    /// Every origin, in variable order.
    pub fn iter(&self) -> impl Iterator<Item = &LocalOrigin> {
        self.origins.iter()
    }

    /// The names one label below a label in front of the suffix, which `*.<suffix>` does not cover.
    pub(crate) fn deeper_names(&self, facts: &InstanceFacts) -> Vec<String> {
        self.origins
            .iter()
            .filter(|origin| matches!(origin.host(), OriginHost::EveryLabelUnder(_)))
            .filter_map(|origin| origin.name(facts))
            .collect()
    }

    /// Every name the instance's leaf is issued for.
    pub(crate) fn leaf_names(&self, facts: &InstanceFacts) -> Vec<String> {
        facts
            .suffixes
            .iter()
            .flat_map(|(_, suffix)| [suffix.to_string(), format!("*.{suffix}")])
            .chain(self.deeper_names(facts))
            .collect()
    }

    /// Whether the project declares no origin, and so runs no edge.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.origins.is_empty()
    }

    /// Every origin's variable and its URL for the instance `facts` describes, in variable order.
    #[must_use]
    pub fn variables(&self, facts: &InstanceFacts) -> Vec<(String, String)> {
        self.origins
            .iter()
            .filter_map(|origin| origin.url(facts).map(|url| (origin.var.to_string(), url)))
            .collect()
    }

    /// What `fun dev` gives `app` on `platform`: every origin, and for each listener the
    /// instance's address, with a TLS posture saying the edge terminates TLS.
    #[must_use]
    pub fn environment(
        &self,
        facts: &InstanceFacts,
        app: &str,
        platform: Platform,
    ) -> BTreeMap<String, String> {
        let mut environment: BTreeMap<String, String> = self.variables(facts).into_iter().collect();
        let listen = platform.app_address(facts.address);
        let prefix = |listener: &str| {
            format!(
                "FUNDAMENTAL_APPS__{}__{}",
                app.to_ascii_uppercase(),
                listener.to_ascii_uppercase()
            )
        };
        for origin in self
            .origins
            .iter()
            .filter(|origin| origin.app.as_str() == app)
        {
            let prefix = prefix(origin.surface.key());
            environment.insert(format!("{prefix}__HOST"), listen.to_string());
            environment.insert(format!("{prefix}__TLS"), TERMINATED_UPSTREAM.to_owned());
        }
        let others = self
            .others
            .iter()
            .filter(|(key, _)| key.as_str() == app)
            .flat_map(|(_, listeners)| listeners);
        for listener in others {
            environment.insert(format!("{}__HOST", prefix(listener)), listen.to_string());
        }
        environment
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use fundamental_core::Error as _;
    use fundamental_manifest::Manifest;

    use crate::{InstanceFacts, InstanceKey, InstanceLabel, InstanceRole, LocalProject};

    fn project(manifest: &str) -> (tempfile::TempDir, Result<LocalProject, crate::LocalError>) {
        let dir = tempfile::tempdir().expect("a directory");
        fs::write(dir.path().join("fun.toml"), manifest).expect("a manifest");
        let loaded = Manifest::discover(dir.path())
            .map_err(crate::LocalError::from)
            .and_then(|manifest| LocalProject::from_manifest(&manifest));
        (dir, loaded)
    }

    const APPS: &str = "[project]\nname = \"acme\"\n\n[apps.www.web]\nport = 3000\n\n[apps.api.api]\nport = 4000\n\n";

    fn facts(project: &LocalProject, role: InstanceRole) -> InstanceFacts {
        let key = InstanceKey::new(project.name().clone(), role);
        InstanceFacts {
            suffixes: project
                .apexes()
                .iter()
                .map(|apex| (apex.clone(), key.role().suffix(apex)))
                .collect(),
            key,
            address: crate::LoopbackAddress::new(7).expect("an address"),
            tls: "/s/tls.pem".into(),
            root: "/s/root.pem".into(),
            fingerprint: crate::RootFingerprint::of_der(&[]),
        }
    }

    #[test]
    fn an_origin_served_at_every_name_under_a_label_is_one_origin_for_main_and_a_lane() {
        let (_dir, loaded) = project(&format!(
            "{APPS}[local]\napexes = [\"acme.test\"]\n\n[local.origins]\nWWW_ORIGIN = {{ app = \"www\", host = \"www\", apex = \"acme.test\" }}\nPREVIEW_ORIGIN = {{ app = \"www\", host = \"*.preview\", apex = \"acme.test\" }}\n"
        ));
        let project = loaded.expect("a wildcard level is an origin");
        let lane = facts(
            &project,
            InstanceRole::Lane(InstanceLabel::parse("fix-login").expect("a label")),
        );

        assert!(project.origins().variables(&lane).contains(&(
            "PREVIEW_ORIGIN".to_owned(),
            "https://*.preview.fix-login.acme-lane.test".to_owned()
        )));
        assert!(
            project
                .origins()
                .variables(&facts(&project, InstanceRole::Main))
                .contains(&(
                    "PREVIEW_ORIGIN".to_owned(),
                    "https://*.preview.acme.test".to_owned()
                ))
        );
    }

    #[test]
    fn the_leaf_is_issued_for_each_suffix_and_each_deeper_wildcard() {
        let (_dir, loaded) = project(&format!(
            "{APPS}[local]\napexes = [\"acme.test\"]\n\n[local.origins]\nWWW_ORIGIN = {{ app = \"www\", host = \"www\", apex = \"acme.test\" }}\nPREVIEW_ORIGIN = {{ app = \"www\", host = \"*.preview\", apex = \"acme.test\" }}\n"
        ));
        let project = loaded.expect("valid origins");
        let lane = facts(
            &project,
            InstanceRole::Lane(InstanceLabel::parse("fix-login").expect("a label")),
        );

        assert_eq!(
            project.origins().leaf_names(&lane),
            [
                "fix-login.acme-lane.test",
                "*.fix-login.acme-lane.test",
                "*.preview.fix-login.acme-lane.test"
            ]
        );
    }

    #[test]
    fn a_host_deeper_than_one_wildcard_level_is_refused_with_the_rest() {
        let (_dir, loaded) = project(&format!(
            "{APPS}[local]\napexes = [\"acme.test\"]\n\n[local.origins]\nPREVIEW_ORIGIN = {{ app = \"www\", host = \"*.*.preview\", apex = \"acme.test\" }}\nDEEP_ORIGIN = {{ app = \"www\", host = \"a.preview\", apex = \"acme.test\" }}\n"
        ));

        let err = loaded.expect_err("refused");

        assert_eq!(err.error_code(), "LOCAL_010");
        let text = err.to_string();
        assert!(
            text.contains("`*.*.preview`") && text.contains("`a.preview`"),
            "{text}"
        );
    }

    #[test]
    fn origins_name_their_app_surface_port_and_apex() {
        let (_dir, loaded) = project(&format!(
            "{APPS}[local]\napexes = [\"acme.test\"]\n\n[local.origins]\nWWW_ORIGIN = {{ app = \"www\", host = \"www\", apex = \"acme.test\" }}\nAPI_ORIGIN = {{ app = \"api\", host = \"api\", surface = \"api\", apex = \"acme.test\" }}\nAPEX_ORIGIN = {{ app = \"www\", apex = \"acme.test\" }}\n"
        ));
        let project = loaded.expect("valid origins");
        let ports: Vec<(String, u16)> = project
            .origins()
            .iter()
            .map(|origin| (origin.var().to_string(), origin.port()))
            .collect();
        assert_eq!(
            ports,
            [
                ("APEX_ORIGIN".to_owned(), 3000),
                ("API_ORIGIN".to_owned(), 4000),
                ("WWW_ORIGIN".to_owned(), 3000)
            ]
        );
        let lane = InstanceRole::Lane(InstanceLabel::parse("fix-login").expect("a label"));
        let lane = facts(&project, lane);
        let main = facts(&project, InstanceRole::Main);
        let vars = project.origins().variables(&lane);
        assert!(vars.contains(&(
            "WWW_ORIGIN".to_owned(),
            "https://www.fix-login.acme-lane.test".to_owned()
        )));
        assert!(
            project
                .origins()
                .variables(&main)
                .contains(&("APEX_ORIGIN".to_owned(), "https://acme.test".to_owned()))
        );
        let env = project
            .origins()
            .environment(&lane, "api", crate::Platform::Linux);
        assert_eq!(
            env.get("FUNDAMENTAL_APPS__API__API__HOST")
                .map(String::as_str),
            Some("127.0.0.7")
        );
        assert_eq!(
            env.get("FUNDAMENTAL_APPS__API__API__TLS")
                .map(String::as_str),
            Some("terminated_upstream")
        );
        assert!(
            env.contains_key("WWW_ORIGIN"),
            "every app is given every origin"
        );
        assert!(
            !env.contains_key("FUNDAMENTAL_APPS__WWW__WEB__HOST"),
            "{env:?}"
        );
    }

    #[test]
    fn every_problem_in_the_origins_is_reported_at_once() {
        let (_dir, loaded) = project(&format!(
            "{APPS}[local]\napexes = [\"acme.test\"]\n\n[local.origins]\nwww = {{ app = \"www\", host = \"w\", apex = \"acme.test\" }}\nAPEX = {{ app = \"www\", apex = \"acme.test\" }}\nFUNDAMENTAL_X = {{ app = \"www\", host = \"x\", apex = \"acme.test\" }}\nGHOST = {{ app = \"ghost\", apex = \"acme.test\" }}\nSYNC = {{ app = \"www\", host = \"s\", surface = \"sync\", apex = \"acme.test\" }}\nNOPORT = {{ app = \"api\", host = \"n\", apex = \"acme.test\" }}\nOTHER = {{ app = \"www\", host = \"o\", apex = \"other.test\" }}\nBADHOST = {{ app = \"www\", host = \"a.b\", apex = \"acme.test\" }}\nTWICE = {{ app = \"api\", surface = \"api\", apex = \"acme.test\" }}\n"
        ));
        let error = loaded.expect_err("eight problems");
        assert_eq!(error.error_code(), "LOCAL_010");
        let message = error.to_string();
        for expected in [
            "`www` is not an environment variable name",
            "`FUNDAMENTAL_X` is in the `FUNDAMENTAL_` namespace",
            "there is no `[apps.ghost]`",
            "surface `sync` cannot be behind the local edge",
            "`[apps.api.web] port` is not declared",
            "apex `other.test` is not one of [local] apexes",
            "host `a.b` is not one lower-case DNS label",
            "another origin is already served at the apex itself",
        ] {
            assert!(
                message.contains(expected),
                "missing {expected:?} in {message}"
            );
        }
    }

    #[test]
    fn an_app_that_is_not_an_app_key_never_reaches_the_edge() {
        let (_dir, loaded) = project(
            "[project]\nname = \"acme\"\n\n[apps.\"www\\n}\\n:9999 {\\n  file_server\"]\n\n[apps.\"www\\n}\\n:9999 {\\n  file_server\".web]\nport = 3000\n\n[apps.My_App.web]\nport = 3001\n\n[local]\napexes = [\"acme.test\"]\n\n[local.origins]\nEVIL = { app = \"www\\n}\\n:9999 {\\n  file_server\", host = \"www\", apex = \"acme.test\" }\nCASED = { app = \"My_App\", host = \"m\", apex = \"acme.test\" }\n",
        );
        let error = loaded.expect_err("neither app is an app key");
        assert_eq!(error.error_code(), "LOCAL_010");
        let message = error.to_string();
        assert!(
            message.contains("`EVIL`: app `www\\n}\\n:9999 {\\n  file_server` is not an app key"),
            "{message}"
        );
        assert!(
            message.contains("`CASED`: app `My_App` is not an app key"),
            "{message}"
        );
        assert!(
            !message.contains("\n  file_server"),
            "the name is never echoed raw: {message}"
        );
    }

    #[test]
    fn a_brand_projects_origin_takes_its_apps_brand() {
        let dir = tempfile::tempdir().expect("a directory");
        fs::write(
            dir.path().join("fun.toml"),
            "[project]\nname = \"dagen\"\n\n[apps.www]\nbrand = \"dagen\"\n\n[apps.www.web]\nport = 3000\n\n[local.brands]\ndagen = \"dagen.test\"\n\n[local.origins]\nWWW_ORIGIN = { app = \"www\", host = \"mijn\" }\nBAD = { app = \"www\", host = \"x\", apex = \"dagen.test\" }\n",
        )
        .expect("a manifest");
        let manifest = Manifest::discover(dir.path()).expect("a manifest");
        let error =
            LocalProject::from_manifest(&manifest).expect_err("apex written in a brand project");
        assert!(
            error.to_string().contains("decided by `[apps.www] brand`"),
            "{error}"
        );

        fs::write(
            dir.path().join("fun.toml"),
            "[project]\nname = \"dagen\"\n\n[apps.www]\nbrand = \"dagen\"\n\n[apps.www.web]\nport = 3000\n\n[local.brands]\ndagen = \"dagen.test\"\n\n[local.origins]\nWWW_ORIGIN = { app = \"www\", host = \"mijn\" }\n",
        )
        .expect("a manifest");
        let manifest = Manifest::discover(dir.path()).expect("a manifest");
        let project = LocalProject::from_manifest(&manifest).expect("the brand decides the apex");
        let apexes: Vec<&str> = project
            .origins()
            .iter()
            .map(|origin| origin.apex().as_str())
            .collect();
        assert_eq!(apexes, ["dagen.test"]);
    }
    #[test]
    fn an_instance_app_key_reaches_the_edge() {
        let dir = tempfile::tempdir().expect("a directory");
        fs::write(
            dir.path().join("fun.toml"),
            "[project]\nname = \"dagen\"\n\n[apps.www]\nbrand = \"dagen\"\nweb = { port = 3000 }\nadmin = { host = \"127.0.0.1\", port = 4000 }\n\n[apps.www_days]\ninstance_of = \"www\"\nbrand = \"days\"\nweb = { port = 3001 }\nadmin = { port = 4001 }\n\n[local.brands]\ndagen = \"dagen.test\"\ndays = \"days.test\"\n\n[local.origins]\nDAGEN_ORIGIN = { app = \"www\" }\nDAYS_ORIGIN = { app = \"www_days\" }\nDAYS_ALIAS = { app = \"www_days\", host = \"www\" }\n",
        )
        .expect("a manifest");
        let manifest = Manifest::discover(dir.path()).expect("a manifest");
        let project = LocalProject::from_manifest(&manifest).expect("an instance is an app key");
        let lane = InstanceRole::Lane(InstanceLabel::parse("add-search").expect("a label"));
        let lane = facts(&project, lane);
        let vars = project.origins().variables(&lane);
        assert!(vars.contains(&(
            "DAYS_ORIGIN".to_owned(),
            "https://add-search.days-lane.test".to_owned()
        )));
        assert!(vars.contains(&(
            "DAYS_ALIAS".to_owned(),
            "https://www.add-search.days-lane.test".to_owned()
        )));
        let ports: Vec<u16> = project
            .origins()
            .iter()
            .filter(|origin| origin.app().as_str() == "www_days")
            .map(|origin| origin.port())
            .collect();
        assert_eq!(ports, [3001, 3001], "the instance's own port");
        let env = project
            .origins()
            .environment(&lane, "www_days", crate::Platform::Linux);
        assert_eq!(
            env.get("FUNDAMENTAL_APPS__WWW_DAYS__WEB__HOST")
                .map(String::as_str),
            Some("127.0.0.7")
        );
        assert!(
            !env.contains_key("FUNDAMENTAL_APPS__WWW__WEB__HOST"),
            "{env:?}"
        );
    }

    #[test]
    fn every_listener_of_an_app_behind_the_edge_listens_on_the_instance_address() {
        let (_dir, loaded) = project(&format!(
            "{APPS}[apps.www.admin]\nhost = \"127.0.0.1\"\nport = 21001\n\n[apps.www.socket]\nport = 5000\n\n[apps.lone.admin]\nport = 21009\n\n[local]\napexes = [\"acme.test\"]\n\n[local.origins]\nWWW_ORIGIN = {{ app = \"www\", apex = \"acme.test\" }}\n"
        ));
        let project = loaded.expect("valid origins");
        let lane = InstanceRole::Lane(InstanceLabel::parse("fix-login").expect("a label"));
        let lane = facts(&project, lane);
        let env = project
            .origins()
            .environment(&lane, "www", crate::Platform::Linux);
        for listener in ["WEB", "ADMIN", "SOCKET"] {
            assert_eq!(
                env.get(&format!("FUNDAMENTAL_APPS__WWW__{listener}__HOST"))
                    .map(String::as_str),
                Some("127.0.0.7"),
                "{listener}: {env:?}"
            );
        }
        assert!(
            !env.contains_key("FUNDAMENTAL_APPS__WWW__ADMIN__TLS"),
            "the edge terminates no listener it does not route to"
        );
        assert!(
            !env.contains_key("FUNDAMENTAL_APPS__WWW__SYNC__HOST"),
            "{env:?}"
        );
        let lone = project
            .origins()
            .environment(&lane, "lone", crate::Platform::Linux);
        assert!(
            !lone.contains_key("FUNDAMENTAL_APPS__LONE__ADMIN__HOST"),
            "an app no origin routes to is left as configured"
        );
    }
}
