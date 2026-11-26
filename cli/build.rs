//! Bakes the commit `fun` was built from into `fun --version`, so two binaries built from
//! different commits say so. A source tree with no git repository still builds, and says it
//! cannot name its revision.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    for path in watched() {
        println!("cargo::rerun-if-changed={}", path.display());
    }
    match describe() {
        Some(build) => println!("cargo::rustc-env=FUNDAMENTAL_BUILD={build}"),
        None => {
            println!(
                "cargo::warning=no git revision available, so `fun --version` cannot say which commit this binary is"
            );
            println!("cargo::rustc-env=FUNDAMENTAL_BUILD=unknown revision");
        }
    }
}

/// The files whose change means a new commit: `HEAD`, its branch's ref and the packed refs. A
/// path that does not exist yet still tells cargo to rerun when it appears.
fn watched() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let Some(head) = git_path("HEAD") else {
        return paths;
    };
    paths.push(head);
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"])
        && let Some(path) = git_path(&reference)
    {
        paths.push(path);
        if let Some(packed) = git_path("packed-refs") {
            paths.push(packed);
        }
    }
    paths
}

/// This build's identity: the commit, `+` when the tree had uncommitted changes, and the date.
fn describe() -> Option<String> {
    let revision = git(&["rev-parse", "--short=8", "HEAD"])?;
    let date = git(&["show", "-s", "--format=%cs", "HEAD"])?;
    let dirty = match git(&["status", "--porcelain"]) {
        Some(status) if !status.is_empty() => "+",
        _ => "",
    };
    Some(format!("{revision}{dirty}, {date}"))
}

/// Where git keeps a file, in a clone or a worktree alike.
fn git_path(name: &str) -> Option<PathBuf> {
    git(&["rev-parse", "--git-path", name]).map(PathBuf::from)
}

/// Git's trimmed answer, or `None` when it gave none.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let answer = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!answer.is_empty()).then_some(answer)
}
