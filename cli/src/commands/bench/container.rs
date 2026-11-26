//! [`Container`]: the pinned Linux machine instruction counts are taken on.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tracing::{info, instrument};

use crate::{CliError, CliResult};

const CONTAINERFILE: &str = include_str!("Containerfile");

/// The gungraun runner installed when the lockfile names no `gungraun`.
const GUNGRAUN: &str = "0.20.0";

/// A container engine and its image, used whenever one is here: callgrind counts libc too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Container {
    engine: String,
    image: String,
    rust: String,
    gungraun: String,
}

impl Container {
    pub(crate) fn find(root: &Path) -> Option<Self> {
        let engine = ["docker", "podman"]
            .into_iter()
            .find(|engine| answers(engine, &["version"]))?;
        let rust = std::fs::read_to_string(root.join("rust-toolchain.toml"))
            .ok()
            .and_then(|text| channel_in(&text))
            .unwrap_or_else(|| "1.96.0".to_owned());
        let gungraun = std::fs::read_to_string(root.join("Cargo.lock"))
            .ok()
            .and_then(|text| gungraun_in_lock(&text))
            .unwrap_or_else(|| GUNGRAUN.to_owned());
        Some(Self {
            engine: engine.to_owned(),
            image: format!("fundamental-bench:{rust}-gungraun-{gungraun}"),
            rust,
            gungraun,
        })
    }

    #[cfg(test)]
    pub(crate) fn example() -> Self {
        Self {
            engine: "docker".to_owned(),
            image: "fundamental-bench:1.96.0-gungraun-0.20.0".to_owned(),
            rust: "1.96.0".to_owned(),
            gungraun: "0.20.0".to_owned(),
        }
    }

    pub(crate) fn image(&self) -> &str {
        &self.image
    }

    /// Builds the image unless the engine already has it.
    ///
    /// # Errors
    /// [`CliError::BenchDidNotRun`] when the build fails.
    #[expect(
        clippy::cognitive_complexity,
        reason = "the span and events expand into branches"
    )]
    #[instrument(skip_all, fields(engine = %self.engine, image = %self.image), err(Debug))]
    pub(crate) fn ensure_image(&self) -> CliResult<()> {
        if answers(&self.engine, &["image", "inspect", &self.image]) {
            return Ok(());
        }
        info!(image = %self.image, "fun.bench.building_image");
        let context = tempfile::tempdir().map_err(|error| did_not_run(error.to_string()))?;
        std::fs::write(context.path().join("Containerfile"), CONTAINERFILE)
            .map_err(|error| did_not_run(error.to_string()))?;
        let status = Command::new(&self.engine)
            .args(["build", "-t", &self.image, "--build-arg"])
            .arg(format!("RUST={}", self.rust))
            .arg("--build-arg")
            .arg(format!("GUNGRAUN={}", self.gungraun))
            .arg("-f")
            .arg(context.path().join("Containerfile"))
            .arg(context.path())
            .status()
            .map_err(|error| {
                did_not_run(format!("{} build cannot be run: {error}", self.engine))
            })?;
        if status.success() {
            Ok(())
        } else {
            Err(did_not_run(format!(
                "{} build of {} exited {status}",
                self.engine, self.image
            )))
        }
    }

    /// Runs `cargo bench <cargo>` over `root` mounted at its own path, so reported paths are the host's.
    pub(crate) fn command(&self, root: &Path, target: &Path, cargo: &[String]) -> Command {
        let mut command = Command::new(&self.engine);
        let host = Host {
            registry: registry(),
            user: user(),
        };
        command.args(self.run_args(root, target, &host, cargo));
        command
    }

    fn run_args(&self, root: &Path, target: &Path, host: &Host, cargo: &[String]) -> Vec<String> {
        let (registry, user) = (&host.registry, host.user.as_str());
        let root = root.display().to_string();
        let home = target.join("cargo").display().to_string();
        let mut args: Vec<String> = [
            "run",
            "--rm",
            // gungraun turns ASLR off through a `personality` call the default seccomp profile refuses.
            "--security-opt",
            "seccomp=unconfined",
            "--user",
            user,
            "-e",
            "HOME=/tmp",
            "-e",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        // Whatever `rust-toolchain.toml` asks: nothing is installed as the unprivileged user.
        args.push(format!("RUSTUP_TOOLCHAIN={}", self.rust));
        args.extend(["-e".to_owned(), format!("CARGO_HOME={home}")]);
        args.extend([
            "-e".to_owned(),
            format!("CARGO_TARGET_DIR={}", target.join("target").display()),
        ]);
        args.extend([
            "-e".to_owned(),
            "PATH=/usr/local/cargo/bin:/usr/bin:/bin".to_owned(),
        ]);
        if let Ok(jobs) = std::env::var("CARGO_BUILD_JOBS") {
            args.extend(["-e".to_owned(), format!("CARGO_BUILD_JOBS={jobs}")]);
        }
        args.extend([
            "-v".to_owned(),
            format!("{}:{home}/registry", registry.display()),
            "-v".to_owned(),
            format!("{root}:{root}"),
            "-w".to_owned(),
            root,
            self.image.clone(),
            "cargo".to_owned(),
            "bench".to_owned(),
        ]);
        args.extend(cargo.iter().cloned());
        args
    }
}

struct Host {
    registry: PathBuf,
    /// `uid:gid`, so what the container builds is the user's to remove.
    user: String,
}

fn channel_in(text: &str) -> Option<String> {
    let table: toml::Table = text.parse().ok()?;
    table
        .get("toolchain")?
        .get("channel")?
        .as_str()
        .map(str::to_owned)
}

fn gungraun_in_lock(text: &str) -> Option<String> {
    let lock: toml::Table = text.parse().ok()?;
    lock.get("package")?
        .as_array()?
        .iter()
        .find(|package| package.get("name").and_then(toml::Value::as_str) == Some("gungraun"))?
        .get("version")?
        .as_str()
        .map(str::to_owned)
}

pub(crate) fn answers(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn registry() -> PathBuf {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
        .unwrap_or_else(|| PathBuf::from(".cargo"))
        .join("registry")
}

fn user() -> String {
    let id = |flag: &str| {
        Command::new("id")
            .arg(flag)
            .output()
            .ok()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| "0".to_owned())
    };
    format!("{}:{}", id("-u"), id("-g"))
}

fn did_not_run(reason: String) -> CliError {
    CliError::BenchDidNotRun { reason }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn container() -> Container {
        Container::example()
    }

    #[test]
    fn the_toolchain_is_the_channel_the_project_pins() {
        let text = "[toolchain]\nchannel = \"1.96.0\"\ncomponents = [\"clippy\"]\n";
        assert_eq!(channel_in(text).as_deref(), Some("1.96.0"));
        assert_eq!(channel_in("[other]\n"), None);
    }

    #[test]
    fn the_runner_is_the_gungraun_the_lockfile_resolved() {
        let lock = "version = 4\n\n[[package]]\nname = \"criterion\"\nversion = \"0.8.1\"\n\n[[package]]\nname = \"gungraun\"\nversion = \"0.20.0\"\n";
        assert_eq!(gungraun_in_lock(lock).as_deref(), Some("0.20.0"));
        assert_eq!(gungraun_in_lock("version = 4\n"), None);
    }

    #[test]
    fn the_workspace_is_mounted_at_its_own_path_and_built_into_its_own_target() {
        let host = Host {
            registry: PathBuf::from("/home/me/.cargo/registry"),
            user: "1000:1000".to_owned(),
        };
        let args = container().run_args(
            Path::new("/w"),
            Path::new("/w/target/bench-container"),
            &host,
            &["--bench".to_owned(), "instructions".to_owned()],
        );
        let joined = args.join(" ");
        assert!(
            joined.starts_with("run --rm --security-opt seccomp=unconfined --user 1000:1000"),
            "{joined}"
        );
        assert!(joined.contains("-v /w:/w -w /w"), "{joined}");
        assert!(
            joined.contains("CARGO_TARGET_DIR=/w/target/bench-container/target"),
            "{joined}"
        );
        assert!(
            joined.contains("-v /home/me/.cargo/registry:/w/target/bench-container/cargo/registry"),
            "{joined}"
        );
        assert!(joined.contains("RUSTUP_TOOLCHAIN=1.96.0"), "{joined}");
        assert!(
            joined.ends_with(
                "fundamental-bench:1.96.0-gungraun-0.20.0 cargo bench --bench instructions"
            ),
            "{joined}"
        );
    }
}
