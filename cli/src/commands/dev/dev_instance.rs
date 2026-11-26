//! The local instance `fun dev` serves its apps behind (`fun local up`).

use std::path::Path;

use fundamental::Error as _;
use fundamental_dev::DevApp;
use fundamental_local::{
    Checkout, InstanceFacts, LocalError, LocalMachine, LocalOrigins, LocalProject, Platform,
};
use time::OffsetDateTime;
use tracing::info;

use crate::CliResult;
use crate::ui::Ui;

/// This checkout's instance, claimed and converged, and the origins its edge serves.
#[derive(Debug, Clone)]
pub(crate) struct DevInstance {
    pub(crate) facts: InstanceFacts,
    pub(crate) origins: LocalOrigins,
    pub(crate) platform: Platform,
}

impl DevInstance {
    /// Runs `fun local up` when the project declares `[local.origins]`; `None`, touching nothing, otherwise.
    ///
    /// # Errors
    /// Any `LOCAL_0xx` refusal of `up`.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) fn up(root: &Path, label: Option<&str>, ui: &Ui) -> CliResult<Option<Self>> {
        let project = match LocalProject::load(root) {
            Ok(project) => project,
            Err(LocalError::NoApexes) => {
                info!("fun dev: the project declares no local origins; serving as configured");
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };
        if project.origins().is_empty() {
            info!(project = %project.name(), "fun dev: the project declares no [local.origins]; serving as configured");
            return Ok(None);
        }
        let machine = LocalMachine::for_user()?;
        let checkout = Checkout::detect(project.root(), label)?;
        let report = machine.up(&project, &checkout, OffsetDateTime::now_utc())?;
        ui.detail(format!(
            "`{}` ({}) serves on {}",
            project.name(),
            checkout.role(),
            report.facts.address
        ));
        for (var, url) in project.origins().variables(&report.facts) {
            ui.detail(format!("{var}={url}"));
        }
        for warning in &report.warnings {
            ui.status(ui.warn(&format!("{}: {warning}", warning.error_code())));
        }
        Ok(Some(Self {
            facts: report.facts,
            origins: project.origins().clone(),
            platform: machine.platform(),
        }))
    }

    /// `dev_app` given the instance's origins, listen addresses and TLS postures, over `.dev.vars.<app>`.
    pub(crate) fn apply(&self, dev_app: DevApp) -> DevApp {
        let environment = self
            .origins
            .environment(&self.facts, &dev_app.name, self.platform);
        dev_app.envs(environment)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use fundamental_dev::DevApp;
    use fundamental_local::{
        Checkout, InstanceLabel, InstanceRole, LocalMachine, LocalProject, LocalState, Platform,
    };
    use time::OffsetDateTime;

    use super::DevInstance;
    use crate::ui::Ui;

    #[test]
    fn fun_dev_claims_nothing_for_a_project_without_origins() {
        let dir = tempfile::tempdir().expect("a directory");
        for manifest in [
            "[project]\nname = \"my_app\"\n\n[apps.www.web]\nport = 3000\n",
            "[project]\nname = \"acme\"\n\n[local]\napexes = [\"acme.test\"]\n",
        ] {
            fs::write(dir.path().join("fun.toml"), manifest).expect("a manifest");
            let up = DevInstance::up(dir.path(), None, &Ui::new(true)).expect("fun dev starts");
            assert!(up.is_none(), "{manifest}");
        }
    }

    #[test]
    fn fun_dev_injects_the_instance_after_dev_vars() {
        let dir = tempfile::tempdir().expect("a directory");
        let root = dir.path().join("checkout");
        fs::create_dir_all(&root).expect("a checkout");
        fs::write(
            root.join("fun.toml"),
            "[project]\nname = \"acme\"\n\n[apps.www.web]\nport = 3000\n\n[local]\napexes = [\"acme.test\"]\n\n[local.origins]\nWWW_ORIGIN = { app = \"www\", host = \"www\", apex = \"acme.test\" }\n",
        )
        .expect("a manifest");
        fs::write(
            root.join(".dev.vars.www"),
            "WWW_ORIGIN=https://www.acme.test\nFUNDAMENTAL_APPS__WWW__WEB__HOST=0.0.0.0\nKEPT=yes\n",
        )
        .expect("the app's own variables");
        let project = LocalProject::load(&root).expect("a project");
        let machine =
            LocalMachine::unmanaged(LocalState::at(dir.path().join("state")), Platform::Linux);
        let checkout = Checkout::new(
            root.canonicalize().expect("real"),
            InstanceRole::Lane(InstanceLabel::parse("feature-a").expect("a label")),
        );
        let up = machine
            .up(&project, &checkout, OffsetDateTime::now_utc())
            .expect("up");
        let instance = DevInstance {
            facts: up.facts,
            origins: project.origins().clone(),
            platform: Platform::Linux,
        };
        let app = DevApp::new("www").envs(fundamental::config::dev_vars_for_app(&root, "www"));
        assert_eq!(
            app.env.get("KEPT").map(String::as_str),
            Some("yes"),
            "{:?}",
            app.env
        );
        let app = instance.apply(app);
        let env = |key: &str| app.env.get(key).map(String::as_str);
        assert_eq!(
            env("WWW_ORIGIN"),
            Some("https://www.feature-a.acme-lane.test")
        );
        assert_eq!(
            env("FUNDAMENTAL_APPS__WWW__WEB__HOST"),
            Some(instance.facts.address.to_string().as_str())
        );
        assert_eq!(
            env("FUNDAMENTAL_APPS__WWW__WEB__TLS"),
            Some("terminated_upstream")
        );
        assert_eq!(
            env("KEPT"),
            Some("yes"),
            "what the instance does not decide is left"
        );
    }
}
