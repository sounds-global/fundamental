//! [`BuildPlan`]: the single cargo invocation that produces every supervised app.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::dev_app::DevApp;

/// What to build: the whole run set in one cargo invocation, so features resolve once.
#[derive(Debug, Clone)]
pub struct BuildPlan {
    apps: Vec<DevApp>,
    release: bool,
    compile_env: Vec<(String, String)>,
}

impl BuildPlan {
    /// A plan that builds `apps`.
    #[must_use]
    pub fn new(apps: Vec<DevApp>) -> Self {
        Self {
            apps,
            release: false,
            compile_env: Vec::new(),
        }
    }

    /// Build with the release profile.
    #[must_use]
    pub fn release(mut self, release: bool) -> Self {
        self.release = release;
        self
    }

    /// Sets `key` to `value` for the compile (`option_env!`).
    #[must_use]
    pub fn compile_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.compile_env.push((key.into(), value.into()));
        self
    }

    /// The variables set for the compile.
    #[must_use]
    pub fn compile_envs(&self) -> &[(String, String)] {
        &self.compile_env
    }

    /// The apps in the plan.
    #[must_use]
    pub fn apps(&self) -> &[DevApp] {
        &self.apps
    }

    /// Whether the plan is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }

    /// The full `cargo` argument vector: every package and every binary in one selection.
    #[must_use]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub fn cargo_args(&self) -> Vec<String> {
        let mut args = vec!["build".to_string()];
        let mut select = |flag: &str, values: Vec<String>| {
            let mut seen = BTreeSet::new();
            for value in values {
                if seen.insert(value.clone()) {
                    args.push(flag.to_string());
                    args.push(value);
                }
            }
        };
        select(
            "-p",
            self.apps.iter().map(|app| app.package.clone()).collect(),
        );
        select(
            "--bin",
            self.apps.iter().map(|app| app.bin.clone()).collect(),
        );
        select(
            "--features",
            self.apps
                .iter()
                .flat_map(|app| {
                    app.features
                        .iter()
                        .map(|feature| format!("{}/{}", app.package, feature))
                })
                .collect(),
        );
        if self.release {
            args.push("--release".to_string());
        }
        args
    }

    /// Where cargo writes `app`'s binary, given the directory cargo builds into.
    #[must_use]
    pub fn binary_path(&self, target_dir: &Path, app: &DevApp) -> PathBuf {
        let profile = if self.release { "release" } else { "debug" };
        target_dir.join(profile).join(&app.bin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> BuildPlan {
        BuildPlan::new(vec![
            DevApp::new("layer0"),
            DevApp::new("layer5_witness").bin("witness"),
        ])
    }

    #[test]
    fn one_invocation_names_every_package_and_binary() {
        let args = plan().cargo_args();
        assert_eq!(args.first().map(String::as_str), Some("build"));
        assert!(args.windows(2).any(|w| w == ["-p", "layer0"]));
        assert!(args.windows(2).any(|w| w == ["-p", "layer5_witness"]));
        assert!(args.windows(2).any(|w| w == ["--bin", "layer0"]));
        assert!(args.windows(2).any(|w| w == ["--bin", "witness"]));
        assert_eq!(args.iter().filter(|a| *a == "build").count(), 1);
    }

    #[test]
    fn one_binary_is_built_once_for_its_instances() {
        let plan = BuildPlan::new(vec![
            DevApp::new("www").feature("introspect"),
            DevApp::new("www_days")
                .package("www")
                .bin("www")
                .feature("introspect"),
            DevApp::new("mijn"),
        ]);
        let args = plan.cargo_args();
        assert_eq!(
            args,
            [
                "build",
                "-p",
                "www",
                "-p",
                "mijn",
                "--bin",
                "www",
                "--bin",
                "mijn",
                "--features",
                "www/introspect"
            ]
        );
    }

    #[test]
    fn auxiliary_binaries_are_not_selected() {
        let args = plan().cargo_args();
        assert!(!args.contains(&"--bins".to_string()));
    }

    #[test]
    fn features_are_package_qualified() {
        let plan = BuildPlan::new(vec![
            DevApp::new("layer0").feature("introspect"),
            DevApp::new("layer5_witness").bin("witness"),
        ]);
        let args = plan.cargo_args();
        assert!(
            args.windows(2)
                .any(|w| w == ["--features", "layer0/introspect"]),
            "{args:?}"
        );
        assert!(
            !args.contains(&"introspect".to_string()),
            "an unqualified feature would apply to every selected package: {args:?}"
        );
    }

    #[test]
    fn release_is_appended_only_when_asked() {
        assert!(
            plan()
                .release(true)
                .cargo_args()
                .contains(&"--release".to_string())
        );
        assert!(!plan().cargo_args().contains(&"--release".to_string()));
        assert!(!plan().cargo_args().contains(&"--features".to_string()));
    }

    #[test]
    fn the_binary_path_follows_the_profile_and_the_target_name() {
        let target = Path::new("/checkouts/a/target");
        let witness = DevApp::new("layer5_witness").bin("witness");

        let debug = plan().binary_path(target, &witness);
        assert_eq!(debug, Path::new("/checkouts/a/target/debug/witness"));

        let release = plan().release(true).binary_path(target, &witness);
        assert_eq!(release, Path::new("/checkouts/a/target/release/witness"));
    }

    #[test]
    fn an_empty_plan_reports_itself_empty() {
        assert!(BuildPlan::new(Vec::new()).is_empty());
        assert!(!plan().is_empty());
    }

    #[test]
    fn a_plan_names_what_its_compile_reads() {
        let plan = BuildPlan::new(vec![DevApp::new("preview_sample")])
            .compile_env("FUNDAMENTAL_FEEDBACK_COMMIT", "457b125");

        assert_eq!(
            plan.compile_envs(),
            [(
                "FUNDAMENTAL_FEEDBACK_COMMIT".to_owned(),
                "457b125".to_owned()
            )]
        );
        assert!(
            !plan.cargo_args().iter().any(|arg| arg.contains("457b125")),
            "a variable, not an argument"
        );
    }
}
