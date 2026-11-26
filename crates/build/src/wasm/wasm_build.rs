//! [`WasmBuild`] — compile a web app's browser bundle behind the wasm-bindgen seam.

use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tracing::{info, instrument, warn};

use super::wasm_artifacts::WasmArtifacts;
use crate::build_error::BuildError;

const WASM_TARGET: &str = "wasm32-unknown-unknown";

const MAX_STDERR: usize = 4000;

/// A browser-bundle build: `cargo build` for `wasm32-unknown-unknown` followed by `wasm-bindgen`.
///
/// Give it a target directory apart from the server build's, such as `<target>/wasm`. The
/// `wasm-bindgen` CLI must match the version of the library the build resolves.
pub struct WasmBuild {
    manifest_path: PathBuf,
    target_dir: PathBuf,
    out_dir: PathBuf,
    crate_name: String,
    out_name: String,
    features: Vec<String>,
    release: bool,
    bindgen: PathBuf,
}

impl WasmBuild {
    /// Creates a build for the crate at `manifest_path` whose library is `crate_name`.
    pub fn new(
        manifest_path: impl Into<PathBuf>,
        target_dir: impl Into<PathBuf>,
        out_dir: impl Into<PathBuf>,
        crate_name: impl Into<String>,
    ) -> Self {
        let crate_name = crate_name.into();
        Self {
            manifest_path: manifest_path.into(),
            target_dir: target_dir.into(),
            out_dir: out_dir.into(),
            out_name: crate_name.clone(),
            crate_name,
            features: Vec::new(),
            release: false,
            bindgen: PathBuf::from("wasm-bindgen"),
        }
    }

    /// Adds a Cargo feature to enable for the wasm build; it always passes `--no-default-features`.
    #[must_use]
    pub fn feature(mut self, feature: impl Into<String>) -> Self {
        self.features.push(feature.into());
        self
    }

    /// Sets the bundle's output name (default: the crate name): `<name>.js` and `<name>_bg.wasm`.
    #[must_use]
    pub fn out_name(mut self, out_name: impl Into<String>) -> Self {
        self.out_name = out_name.into();
        self
    }

    /// Binds with the `wasm-bindgen` CLI at `cli` (default: the one first on `PATH`).
    #[must_use]
    pub fn bindgen(mut self, cli: impl Into<PathBuf>) -> Self {
        self.bindgen = cli.into();
        self
    }

    /// Builds with the release profile (optimized, no debug assertions).
    #[must_use]
    pub fn release(mut self, release: bool) -> Self {
        self.release = release;
        self
    }

    /// Compiles the crate to wasm and runs `wasm-bindgen`, returning the bundle.
    ///
    /// # Errors
    /// - [`BuildError::ToolLaunch`] if `cargo` or `wasm-bindgen` cannot be launched.
    /// - [`BuildError::ToolFailed`] if either tool exits non-zero.
    #[instrument(
        skip(self),
        fields(crate_name = %self.crate_name, release = self.release),
        err(Debug)
    )]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn build(&self) -> Result<WasmArtifacts, BuildError> {
        info!("compiling browser bundle to wasm");
        run("cargo", &self.cargo_args())?;

        let wasm = self.compiled_wasm();
        info!(wasm = %wasm.display(), "running wasm-bindgen");
        run(&self.bindgen.to_string_lossy(), &self.bindgen_args(&wasm))?;

        Ok(WasmArtifacts {
            wasm: self.out_dir.join(format!("{}_bg.wasm", self.out_name)),
            js: self.out_dir.join(format!("{}.js", self.out_name)),
        })
    }

    /// The `cargo rustc` arguments.
    ///
    /// Its own target directory, so it never waits on the server build's lock; the `cdylib`
    /// crate type is asked for here so the app's native builds never link one.
    fn cargo_args(&self) -> Vec<String> {
        let mut args = vec![
            "rustc".to_owned(),
            "--manifest-path".to_owned(),
            self.manifest_path.display().to_string(),
            "--target-dir".to_owned(),
            self.target_dir.display().to_string(),
            "--target".to_owned(),
            WASM_TARGET.to_owned(),
            "--lib".to_owned(),
            "--crate-type".to_owned(),
            "cdylib".to_owned(),
            "--no-default-features".to_owned(),
        ];
        if self.release {
            args.push("--release".to_owned());
        }
        for feature in &self.features {
            args.push("--features".to_owned());
            args.push(feature.clone());
        }
        args
    }

    /// Where cargo records this build's source set, its `.d` dep-info file, which exists
    /// after the first successful build.
    #[must_use]
    pub fn dep_info_path(&self) -> PathBuf {
        let profile = if self.release { "release" } else { "debug" };
        self.target_dir
            .join(WASM_TARGET)
            .join(profile)
            .join(format!("{}.d", self.crate_name))
    }

    fn compiled_wasm(&self) -> PathBuf {
        let profile = if self.release { "release" } else { "debug" };
        self.target_dir
            .join(WASM_TARGET)
            .join(profile)
            .join(format!("{}.wasm", self.crate_name))
    }

    fn bindgen_args(&self, wasm: &Path) -> Vec<String> {
        vec![
            "--target".to_owned(),
            "web".to_owned(),
            "--out-dir".to_owned(),
            self.out_dir.display().to_string(),
            "--out-name".to_owned(),
            self.out_name.clone(),
            "--no-typescript".to_owned(),
            wasm.display().to_string(),
        ]
    }
}

/// Runs `tool` with `args`, streaming its stderr live and keeping its tail for the error.
///
/// The single piped stream is read to EOF before `wait`, which avoids a pipe deadlock.
fn run(tool: &str, args: &[String]) -> Result<(), BuildError> {
    let launch_error = |source| BuildError::ToolLaunch {
        tool: tool.to_owned(),
        source,
    };

    let mut child = Command::new(tool)
        .args(args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(launch_error)?;

    let mut tail = TailBuffer::new(MAX_STDERR);
    if let Some(stderr) = child.stderr.take() {
        let mut sink = io::stderr();
        for line in BufReader::new(stderr).lines() {
            let line = line.map_err(launch_error)?;
            // A write hiccup on our stderr must not abort the build.
            let _ = writeln!(sink, "{line}");
            tail.push_line(&line);
        }
    }

    let status = child.wait().map_err(launch_error)?;
    if !status.success() {
        warn!(
            tool,
            code = status.code().unwrap_or(-1),
            "build tool exited non-zero"
        );
        return Err(BuildError::ToolFailed {
            tool: tool.to_owned(),
            message: tail.into_message(),
        });
    }
    Ok(())
}

/// The last `cap` bytes of a tool's stderr, marking whether anything was dropped.
struct TailBuffer {
    cap: usize,
    buf: String,
    truncated: bool,
}

impl TailBuffer {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            buf: String::new(),
            truncated: false,
        }
    }

    fn push_line(&mut self, line: &str) {
        if !self.buf.is_empty() {
            self.buf.push('\n');
        }
        self.buf.push_str(line);
        if self.buf.len() <= self.cap {
            return;
        }
        let mut cut = self.buf.len() - self.cap;
        while !self.buf.is_char_boundary(cut) {
            cut += 1;
        }
        self.buf.replace_range(..cut, "");
        self.truncated = true;
    }

    fn into_message(self) -> String {
        let trimmed = self.buf.trim();
        if self.truncated {
            format!("…{trimmed}")
        } else {
            trimmed.to_owned()
        }
    }
}

/// Reports the installed `wasm-bindgen` CLI version.
///
/// # Errors
/// [`BuildError::ToolLaunch`] if the `wasm-bindgen` CLI is not installed.
pub fn bindgen_cli_version() -> Result<String, BuildError> {
    let output = Command::new("wasm-bindgen")
        .arg("--version")
        .output()
        .map_err(|source| BuildError::ToolLaunch {
            tool: "wasm-bindgen".to_owned(),
            source,
        })?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build() -> WasmBuild {
        WasmBuild::new("/app/Cargo.toml", "/app/target", "/app/dist", "acme").feature("hydrate")
    }

    #[test]
    fn cargo_args_target_the_wasm_library_with_no_defaults() {
        let args = build().cargo_args();
        assert!(args.windows(2).any(|w| w == ["--target", WASM_TARGET]));
        assert!(args.contains(&"--lib".to_owned()));
        assert_eq!(args[0], "rustc");
        assert!(args.windows(2).any(|w| w == ["--crate-type", "cdylib"]));
        assert!(args.contains(&"--no-default-features".to_owned()));
        assert!(args.windows(2).any(|w| w == ["--features", "hydrate"]));
        assert!(!args.contains(&"--release".to_owned()));
    }

    #[test]
    fn cargo_builds_in_the_wasm_builds_own_target_dir() {
        let args = build().cargo_args();
        assert!(
            args.windows(2)
                .any(|w| w == ["--target-dir", "/app/target"]),
            "{args:?}"
        );
    }

    #[test]
    fn release_adds_the_release_flag_and_path() {
        let args = build().release(true).cargo_args();
        assert!(args.contains(&"--release".to_owned()));
        let wasm = build().release(true).compiled_wasm();
        assert!(wasm.ends_with("wasm32-unknown-unknown/release/acme.wasm"));
    }

    #[test]
    fn compiled_wasm_path_uses_the_crate_name_and_profile() {
        let wasm = build().compiled_wasm();
        assert!(wasm.ends_with("wasm32-unknown-unknown/debug/acme.wasm"));
    }

    #[test]
    fn dep_info_sits_beside_the_wasm_for_the_same_profile() {
        let debug = build().dep_info_path();
        assert!(
            debug.ends_with("wasm32-unknown-unknown/debug/acme.d"),
            "{debug:?}"
        );
        let release = build().release(true).dep_info_path();
        assert!(
            release.ends_with("wasm32-unknown-unknown/release/acme.d"),
            "{release:?}"
        );
    }

    #[test]
    fn bindgen_args_emit_an_es_module_bundle() {
        let b = build();
        let args = b.bindgen_args(Path::new("/app/target/in.wasm"));
        assert!(args.windows(2).any(|w| w == ["--target", "web"]));
        assert!(args.windows(2).any(|w| w == ["--out-name", "acme"]));
        assert!(args.contains(&"--no-typescript".to_owned()));
        assert!(args.contains(&"/app/target/in.wasm".to_owned()));
    }

    #[test]
    fn the_bindgen_cli_is_the_one_on_path_unless_one_is_named() {
        assert_eq!(build().bindgen, Path::new("wasm-bindgen"));
        let named = build().bindgen("/cache/wasm-bindgen-cli/0.2.122/bin/wasm-bindgen");
        assert_eq!(
            named.bindgen,
            Path::new("/cache/wasm-bindgen-cli/0.2.122/bin/wasm-bindgen")
        );
    }

    #[test]
    fn out_name_overrides_the_bundle_name() {
        let args = build()
            .out_name("bundle")
            .bindgen_args(Path::new("/x.wasm"));
        assert!(args.windows(2).any(|w| w == ["--out-name", "bundle"]));
    }

    #[test]
    fn launching_a_missing_tool_is_a_tool_launch_error() {
        use fundamental_core::Error as _;
        let err = run("definitely-not-a-real-tool-xyz", &["--version".to_owned()])
            .expect_err("missing tool");
        assert_eq!(err.error_code(), "BUILD_008");
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_tool_captures_its_stderr_tail() {
        use fundamental_core::Error as _;
        let err = run(
            "sh",
            &["-c".to_owned(), "echo boom 1>&2; exit 3".to_owned()],
        )
        .expect_err("non-zero exit");
        assert_eq!(err.error_code(), "BUILD_009");
        match err {
            BuildError::ToolFailed { tool, message } => {
                assert_eq!(tool, "sh");
                assert!(message.contains("boom"), "missing stderr: {message:?}");
            }
            other => panic!("expected ToolFailed, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_succeeding_tool_returns_ok() {
        run("sh", &["-c".to_owned(), "exit 0".to_owned()]).expect("clean exit");
    }

    #[test]
    fn tail_buffer_keeps_the_end_and_flags_truncation() {
        let mut tail = TailBuffer::new(8);
        tail.push_line("first");
        tail.push_line("second");
        let message = tail.into_message();
        assert!(message.starts_with('…'), "{message:?}");
        assert!(message.contains("second"), "{message:?}");
        assert!(!message.contains("first"), "{message:?}");
    }

    #[test]
    fn tail_buffer_under_cap_is_verbatim() {
        let mut tail = TailBuffer::new(64);
        tail.push_line("only line");
        assert_eq!(tail.into_message(), "only line");
    }

    #[test]
    fn tail_buffer_trims_to_a_char_boundary() {
        // Each `é` is two bytes; the cut must land on a UTF-8 boundary.
        let mut tail = TailBuffer::new(5);
        tail.push_line("ééééé");
        let message = tail.into_message();
        assert!(message.starts_with('…'), "{message:?}");
        assert!(
            message.chars().filter(|c| *c == 'é').count() <= 2,
            "{message:?}"
        );
    }
}
