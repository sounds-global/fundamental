//! The page checks and the dependency rules, each red on a planted fixture project.

use std::fs;
use std::path::PathBuf;

use fundamental_build::{BuildError, PagesBuild};
use fundamental_core::Error as _;
use tempfile::TempDir;

const PAGE: &str = "pub fn page() {}\n";

/// Rules every clean fixture keeps.
const TABLE: &str = "[[dependency_rules]]\ncrate = \"domain\"\nmust_not_depend_on = [\"store\", \"ui\"]\n\n[[dependency_rules]]\ncrate = \"app\"\nmust_not_depend_on = [\"store\"]\n";

fn pending(file: &str, rule: &str, count: u32) -> String {
    format!(
        "\n[[dependency_check_pending]]\nfile = \"{file}\"\nrule = \"{rule}\"\ncount = {count}\nuntil = \"issue-51\"\n"
    )
}

const APP_DEPENDENCIES: &str = "[dependencies]\ndomain = { path = \"../crates/domain\" }\nui = { path = \"../crates/ui\" }\n\n[dev-dependencies]\nstore = { path = \"../crates/store\" }\n";

struct Project {
    dir: TempDir,
}

impl Project {
    fn new() -> Self {
        let project = Self {
            dir: TempDir::new().unwrap(),
        };
        project.write(
            "fun.toml",
            &format!("[project]\nname = \"demo\"\n\n{TABLE}"),
        );
        project.write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\", \"app\"]\nresolver = \"3\"\n",
        );
        project.write("crates/domain/Cargo.toml", &package("domain", ""));
        project.write(
            "crates/ui/Cargo.toml",
            &package("ui", "[dependencies]\ndomain = { path = \"../domain\" }\n"),
        );
        project.write(
            "crates/store/Cargo.toml",
            &package(
                "store",
                "[dependencies]\ndomain = { path = \"../domain\" }\n",
            ),
        );
        project.write("app/Cargo.toml", &package("app", APP_DEPENDENCIES));
        project.write("app/src/lib.rs", "");
        project.write("app/pages/index.rs", PAGE);
        project
    }

    fn write(&self, rel: &str, body: &str) {
        let path = self.dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn table(&self, extra: &str) {
        self.write(
            "fun.toml",
            &format!("[project]\nname = \"demo\"\n\n{TABLE}{extra}"),
        );
    }

    fn build(&self) -> Result<(), BuildError> {
        let out = self.dir.path().join("out");
        fs::create_dir_all(&out).unwrap();
        PagesBuild::new(self.dir.path().join("app"), out).run()
    }

    fn refusal(&self) -> Vec<(&'static str, String)> {
        match self.build() {
            Err(BuildError::ArchitectureRefused {
                count, findings, ..
            }) => {
                assert_eq!(count, findings.len());
                findings
                    .iter()
                    .map(|f| (f.error_code(), f.to_string()))
                    .collect()
            }
            other => panic!("expected BUILD_036, got {other:?}"),
        }
    }

    fn generated(&self, name: &str) -> PathBuf {
        self.dir.path().join("out").join(name)
    }
}

fn package(name: &str, rest: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n{rest}")
}

fn codes(findings: &[(&'static str, String)]) -> Vec<&'static str> {
    findings.iter().map(|(code, _)| *code).collect()
}

fn mentions(findings: &[(&'static str, String)], code: &str, text: &str) -> bool {
    findings.iter().any(|(c, f)| *c == code && f.contains(text))
}

#[test]
fn a_clean_project_builds_and_generates_its_pages() {
    let project = Project::new();
    project.build().unwrap();
    for name in ["pages.rs", "hydrate.rs", "addresses.rs"] {
        assert!(project.generated(name).is_file(), "{name}");
    }
}

#[test]
fn cfg_in_a_view_is_refused() {
    let project = Project::new();
    project.write(
        "app/pages/index.rs",
        "#[cfg(feature = \"ssr\")]\nfn helper() {}\npub fn page() {}\n",
    );
    project.write(
        "app/src/team/present.rs",
        "pub fn present() -> bool {\n    cfg!(feature = \"ssr\")\n}\n",
    );
    let findings = project.refusal();
    assert_eq!(codes(&findings), ["BUILD_027", "BUILD_027"], "{findings:?}");
    assert!(mentions(&findings, "BUILD_027", "app/pages/index.rs:1"));
    assert!(mentions(
        &findings,
        "BUILD_027",
        "app/src/team/present.rs:2"
    ));
    assert!(
        !project.generated("pages.rs").exists(),
        "nothing is generated around a refusal"
    );
}

#[test]
fn async_in_a_view_is_refused() {
    let project = Project::new();
    project.write(
        "app/pages/layout.rs",
        "pub fn layout() {}\n\nasync fn load() {}\n",
    );
    project.write(
        "app/pages/index.rs",
        "pub fn page() {\n    view! { <button on:click=move |_| spawn(async move { save().await })/> }\n}\n",
    );
    let findings = project.refusal();
    assert_eq!(codes(&findings).len(), 3, "{findings:?}");
    assert!(codes(&findings).iter().all(|code| *code == "BUILD_028"));
    assert!(mentions(&findings, "BUILD_028", "app/pages/layout.rs:3"));
    assert!(mentions(&findings, "BUILD_028", "app/pages/index.rs:2"));
}

#[test]
fn without_rules_the_page_checks_still_run() {
    let project = Project::new();
    project.write("fun.toml", "[project]\nname = \"demo\"\n");
    project.write(
        "app/pages/index.rs",
        "#[cfg(x)]\nasync fn a() {}\npub fn page() {}\n",
    );
    let findings = project.refusal();
    assert_eq!(codes(&findings), ["BUILD_027", "BUILD_028"], "{findings:?}");

    project.write("app/pages/index.rs", PAGE);
    project.build().unwrap();
    assert!(project.generated("pages.rs").is_file());
}

#[test]
fn a_rule_that_holds_is_no_finding() {
    let project = Project::new();
    project.table("\n[[dependency_rules]]\ncrate = \"ui\"\nmust_not_depend_on = [\"store\"]\n");
    project.build().unwrap();
}

#[test]
fn a_forbidden_crate_named_in_a_view_is_refused() {
    let project = Project::new();
    project.write(
        "app/Cargo.toml",
        &package(
            "app",
            &APP_DEPENDENCIES.replace(
                "\n\n[dev-dependencies]",
                "\nstore = { path = \"../crates/store\" }\n\n[dev-dependencies]",
            ),
        ),
    );
    project.write(
        "app/pages/index.rs",
        "use store::Repo;\nuse store as s;\nextern crate store;\nuse domain::Label;\nuse ui::Card;\npub fn page() {\n    ::store::read();\n    view! { <Card>{store::count()}</Card> }\n}\n",
    );
    let findings = project.refusal();
    let mut found = codes(&findings);
    found.sort_unstable();
    assert_eq!(
        found,
        ["BUILD_029"; 5]
            .iter()
            .copied()
            .chain(["BUILD_030"])
            .collect::<Vec<_>>(),
        "{findings:?}"
    );
    for line in [1, 2, 3, 7, 8] {
        assert!(
            mentions(
                &findings,
                "BUILD_029",
                &format!("app/pages/index.rs:{line}: view names forbidden dependency `store`")
            ),
            "line {line}: {findings:?}"
        );
    }
}

#[test]
fn a_forbidden_crate_edge_is_refused() {
    let project = Project::new();
    project.write(
        "crates/domain/Cargo.toml",
        &package(
            "domain",
            "[dependencies]\nstore = { path = \"../store\" }\n",
        ),
    );
    project.write("crates/store/Cargo.toml", &package("store", ""));
    let findings = project.refusal();
    assert_eq!(codes(&findings), ["BUILD_030"], "{findings:?}");
    assert!(mentions(
        &findings,
        "BUILD_030",
        "crates/domain/Cargo.toml:7: dependency rule forbids `domain` → `store`"
    ));
}

#[test]
fn a_dev_dependency_is_not_an_edge() {
    let project = Project::new();
    project.write(
        "crates/domain/Cargo.toml",
        &package(
            "domain",
            "[dev-dependencies]\nstore = { path = \"../store\" }\n",
        ),
    );
    project.build().unwrap();
}

#[test]
fn a_route_beside_its_directory_is_refused_without_a_policy() {
    let project = Project::new();
    project.write("app/pages/staff.rs", PAGE);
    project.write("app/pages/staff/vooraf.rs", PAGE);
    project.write("app/pages/teams/index.rs", PAGE);
    project.write("app/pages/teams/new.rs", PAGE);
    let findings = project.refusal();
    assert_eq!(codes(&findings), ["BUILD_032"], "{findings:?}");
    assert!(mentions(
        &findings,
        "BUILD_032",
        "app/pages/staff.rs:1: this page sits beside the directory 'app/pages/staff'"
    ));
}

#[test]
fn a_view_pulling_in_hidden_source_is_refused() {
    let project = Project::new();
    project.write(
        "app/pages/index.rs",
        "mod parts;\n#[path = \"other.rs\"]\nmod other {}\nconst X: &str = include!(\"x.rs\");\npub fn page() {}\n",
    );
    let findings = project.refusal();
    assert_eq!(codes(&findings), ["BUILD_033"; 3], "{findings:?}");
    for what in ["`mod parts;`", "`#[path]`", "`include!`"] {
        assert!(
            mentions(&findings, "BUILD_033", what),
            "{what}: {findings:?}"
        );
    }
}

#[test]
fn a_cfg_test_module_is_allowed() {
    let project = Project::new();
    project.write(
        "app/pages/index.rs",
        "pub fn page() {}\n\n#[cfg(test)]\nmod tests {\n    use store::Repo;\n    #[tokio::test]\n    async fn renders() {}\n}\n",
    );
    project.build().unwrap();
}

#[test]
fn a_pending_entry_holds_exactly_its_count() {
    let project = Project::new();
    project.write(
        "app/pages/index.rs",
        "#[cfg(feature = \"ssr\")]\nfn a() {}\n#[cfg(feature = \"ssr\")]\nfn b() {}\npub fn page() {}\n",
    );

    project.table(&pending("app/pages/index.rs", "BUILD_027", 2));
    project.build().unwrap();

    project.table(&pending("app/pages/index.rs", "BUILD_027", 1));
    let more = project.refusal();
    assert_eq!(
        codes(&more),
        ["BUILD_034", "BUILD_027", "BUILD_027"],
        "{more:?}"
    );
    assert!(mentions(
        &more,
        "BUILD_034",
        "freezes 1 BUILD_027 finding(s) and found 2"
    ));

    project.table(&pending("app/pages/index.rs", "BUILD_027", 3));
    let fewer = project.refusal();
    assert_eq!(codes(&fewer), ["BUILD_034"], "{fewer:?}");
    assert!(mentions(&fewer, "BUILD_034", "Lower the count to 2"));

    project.table(&pending("app/pages/gone.rs", "BUILD_027", 2));
    let missing = project.refusal();
    assert!(
        mentions(&missing, "BUILD_034", "there is no such file"),
        "{missing:?}"
    );
    assert!(
        codes(&missing).contains(&"BUILD_027"),
        "the unfrozen findings stand: {missing:?}"
    );
}

#[test]
fn an_invalid_rule_is_refused() {
    let project = Project::new();
    project.table("\n[[dependency_rules]]\ncrate = \"\"\nmust_not_depend_on = [\"x\"]\n");
    let findings = project.refusal();
    assert_eq!(codes(&findings), ["BUILD_035"], "{findings:?}");
    assert!(mentions(
        &findings,
        "BUILD_035",
        "has an invalid `crate` name"
    ));

    project.table(
        "\n[[dependency_rules]]\ncrate = \"ui\"\nmust_not_depend_on = [\"store\", \"store\"]\n",
    );
    let findings = project.refusal();
    assert_eq!(codes(&findings), ["BUILD_035"], "{findings:?}");
    assert!(mentions(
        &findings,
        "BUILD_035",
        "`store` is listed more than once for `ui`"
    ));

    project.table("\n[[dependency_rules]]\ncrate = \"ghost\"\nmust_not_depend_on = [\"store\"]\n");
    let findings = project.refusal();
    assert_eq!(codes(&findings), ["BUILD_035"], "{findings:?}");
    assert!(mentions(
        &findings,
        "BUILD_035",
        "`ghost` is not a workspace crate"
    ));
}

#[test]
fn every_finding_is_collected() {
    let project = Project::new();
    project.write(
        "crates/domain/Cargo.toml",
        &package(
            "domain",
            "[dependencies]\nstore = { path = \"../store\" }\n",
        ),
    );
    project.write("crates/store/Cargo.toml", &package("store", ""));
    project.write(
        "app/pages/index.rs",
        "#[cfg(x)]\nfn a() {}\nasync fn b() {}\npub fn page() {}\n",
    );
    let findings = project.refusal();
    let mut found = codes(&findings);
    found.sort_unstable();
    assert_eq!(
        found,
        ["BUILD_027", "BUILD_028", "BUILD_030"],
        "{findings:?}"
    );
    let refusal = project.build().unwrap_err();
    assert_eq!(refusal.error_code(), "BUILD_036");
    let message = refusal.to_string();
    assert!(message.contains("3 finding(s)"), "{message}");
    for code in ["BUILD_027", "BUILD_028", "BUILD_030"] {
        assert!(
            message.contains(&format!("\n  {code} ")),
            "{code} listed: {message}"
        );
    }
}

#[test]
fn a_finding_in_another_apps_file_is_that_apps_to_judge() {
    let project = Project::new();
    project.table(&pending("other/pages/index.rs", "BUILD_027", 1));
    project.write(
        "other/pages/index.rs",
        "#[cfg(x)]\nfn a() {}\npub fn page() {}\n",
    );
    project.build().unwrap();
}

#[test]
fn a_manifest_between_the_app_and_the_project_s_own_is_refused() {
    let project = Project::new();
    project.write("app/fun.toml", "[project]\nname = \"app\"\n");
    project.write(
        "app/pages/index.rs",
        "#[cfg(x)]\nfn a() {}\npub fn page() {}\n",
    );
    let findings = project.refusal();
    assert_eq!(codes(&findings), ["BUILD_035"], "{findings:?}");
    assert!(mentions(
        &findings,
        "BUILD_035",
        "stands between the app and the project's own"
    ));
}

#[test]
fn a_forbidden_edge_is_frozen_by_the_crate_it_reaches() {
    let project = Project::new();
    project.write(
        "crates/domain/Cargo.toml",
        &package(
            "domain",
            "[dependencies]\nstore = { path = \"../store\" }\n",
        ),
    );
    project.write("crates/store/Cargo.toml", &package("store", ""));
    let frozen = |krate: &str| {
        format!(
            "\n[[dependency_check_pending]]\nfile = \"crates/domain/Cargo.toml\"\nrule = \"BUILD_030\"\ncrate = \"{krate}\"\ncount = 1\nuntil = \"issue-51\"\n"
        )
    };
    project.table(&frozen("store"));
    project.build().unwrap();

    project.table(&frozen("ui"));
    let findings = project.refusal();
    let mut found = codes(&findings);
    found.sort_unstable();
    assert_eq!(found, ["BUILD_030", "BUILD_034"], "{findings:?}");
}
