//! The pinned Tailwind CSS standalone binary a web app's `css_command` runs, checked against
//! the release's published digest before every use.

use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::BuildError;

type Result<T> = std::result::Result<T, BuildError>;

/// The Tailwind release every project's stylesheet is built with.
pub const VERSION: &str = "4.1.11";

/// SHA-256 of each platform's standalone binary, as the release's `sha256sums.txt` publishes it.
const DIGESTS: [(&str, &str); 4] = [
    (
        "linux-arm64",
        "0409aa4222969f47fa6f4160fe5387e79bf7269e7afe0e8b22f7532c98e1d314",
    ),
    (
        "linux-x64",
        "64805b84af4292e043ea6f86d242f191c0ac75359c1a498455dfe6c642afdbab",
    ),
    (
        "macos-arm64",
        "f5984b9c005c3e67841c33906c7a7c92e85e405f61e029e9bb62e880dd662e79",
    ),
    (
        "macos-x64",
        "76e27326506d10d50e65b751795f0537f9304ecb100abe835ec138c41774f38c",
    ),
];

/// The directory holding the pinned `tailwindcss`: an installed one whose digest matches,
/// otherwise the cached copy, fetched when missing.
///
/// # Errors
/// [`BuildError::Tailwind`] when this platform has no pinned binary, no cache directory is known, the
/// download fails, or the binary's digest is not the published one.
pub fn pinned_dir() -> Result<PathBuf> {
    let platform = platform(std::env::consts::OS, std::env::consts::ARCH)?;
    let digest = digest_for(&platform)?;
    let env_bin = std::env::var_os("TAILWINDCSS_BIN");
    let path = std::env::var_os("PATH");
    if let Some(dir) = installed_dir(env_bin.as_deref(), path.as_deref(), digest) {
        return Ok(dir);
    }
    let dir = cache_root()?.join("tailwindcss").join(VERSION);
    let binary = dir.join("tailwindcss");
    if binary.is_file() && digest_of(&binary)? == digest {
        return Ok(dir);
    }
    fetch(&platform, digest, &dir)?;
    Ok(dir)
}

/// Builds the stylesheet `input` describes into `output` with the pinned `tailwindcss`, its
/// `@source` lines read from `input`'s directory.
///
/// # Errors
/// [`BuildError::Tailwind`] when the pinned binary cannot be had (see [`pinned_dir`]), or when
/// it cannot run or refuses the input.
#[tracing::instrument(skip_all, fields(input = %input.display()))]
pub fn compile(input: &Path, output: &Path) -> Result<()> {
    run(&pinned_dir()?.join("tailwindcss"), input, output)
}

/// The stylesheet of the utilities `sources` name, over Tailwind's own theme, without preflight.
///
/// # Errors
/// As [`compile`], and [`BuildError::Tailwind`] when the scratch directory cannot be written
/// or the stylesheet read back.
pub fn utilities(sources: &[&Path]) -> Result<String> {
    static RUN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fundamental-tailwind-{}-{}",
        std::process::id(),
        RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).map_err(|e| io(&dir.display().to_string(), e.to_string()))?;
    let lines: String = sources
        .iter()
        .map(|source| format!("@source \"{}\";\n", source.display()))
        .collect();
    let input = dir.join("input.css");
    let output = dir.join("output.css");
    std::fs::write(
        &input,
        format!(
            "@import \"tailwindcss/theme.css\";\n@import \"tailwindcss/utilities.css\";\n{lines}"
        ),
    )
    .map_err(|e| io(&input.display().to_string(), e.to_string()))?;
    compile(&input, &output)?;
    let css = std::fs::read_to_string(&output)
        .map_err(|e| io(&output.display().to_string(), e.to_string()));
    if let Err(error) = std::fs::remove_dir_all(&dir) {
        tracing::warn!(dir = %dir.display(), %error, "the scratch directory was left behind");
    }
    css
}

fn run(binary: &Path, input: &Path, output: &Path) -> Result<()> {
    let ran = Command::new(binary)
        .arg("--input")
        .arg(input)
        .arg("--output")
        .arg(output)
        .current_dir(input.parent().unwrap_or_else(|| Path::new(".")))
        .output()
        .map_err(|e| {
            io(
                &binary.display().to_string(),
                format!("could not run it: {e}"),
            )
        })?;
    if !ran.status.success() {
        let said = String::from_utf8_lossy(&ran.stderr);
        return Err(io(
            &input.display().to_string(),
            format!("tailwindcss refused it: {}", said.trim()),
        ));
    }
    tracing::debug!(output = %output.display(), "stylesheet compiled");
    Ok(())
}

/// The directory of an installed `tailwindcss` whose SHA-256 is `digest`: `env_bin` first, then
/// each `PATH` entry; `env_bin` counts only when its file is named `tailwindcss`.
fn installed_dir(
    env_bin: Option<&std::ffi::OsStr>,
    path: Option<&std::ffi::OsStr>,
    digest: &str,
) -> Option<PathBuf> {
    let named = env_bin
        .map(PathBuf::from)
        .filter(|bin| bin.file_name().is_some_and(|name| name == "tailwindcss"))
        .and_then(|bin| bin.parent().map(Path::to_path_buf));
    named
        .into_iter()
        .chain(
            path.map(|p| std::env::split_paths(p).collect::<Vec<_>>())
                .unwrap_or_default(),
        )
        .find(|dir| {
            let binary = dir.join("tailwindcss");
            binary.is_file() && digest_of(&binary).is_ok_and(|d| d == digest)
        })
}

fn platform(os: &str, arch: &str) -> Result<String> {
    let os = match os {
        "linux" => "linux",
        "macos" => "macos",
        other => return Err(unsupported(other, arch)),
    };
    let arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => return Err(unsupported(os, other)),
    };
    Ok(format!("{os}-{arch}"))
}

fn digest_for(platform: &str) -> Result<&'static str> {
    DIGESTS
        .iter()
        .find(|(pinned, _)| *pinned == platform)
        .map(|(_, digest)| *digest)
        .ok_or_else(|| {
            io(
                "tailwindcss",
                format!("no pinned {VERSION} binary for {platform}"),
            )
        })
}

fn unsupported(os: &str, arch: &str) -> BuildError {
    io(
        "tailwindcss",
        format!("Tailwind {VERSION} has no standalone binary pinned for {os} on {arch}"),
    )
}

fn cache_root() -> Result<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .map(|base| base.join("fundamental"))
        .ok_or_else(|| {
            io(
                "tailwindcss",
                "neither XDG_CACHE_HOME nor HOME names a cache directory".into(),
            )
        })
}

/// Downloads the release binary and moves it in only once its digest is the published one.
fn fetch(platform: &str, digest: &str, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(|e| io(&dir.display().to_string(), e.to_string()))?;
    let url = format!(
        "https://github.com/tailwindlabs/tailwindcss/releases/download/v{VERSION}/tailwindcss-{platform}"
    );
    let partial = partial_in(dir);
    let status = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--retry",
            "2",
            "--output",
        ])
        .arg(&partial)
        .arg(&url)
        .status()
        .map_err(|e| io("curl", format!("could not run curl to fetch {url}: {e}")))?;
    if !status.success() {
        let _ = std::fs::remove_file(&partial);
        return Err(io(
            "curl",
            format!("could not fetch {url}; check the network and run it again"),
        ));
    }
    let fetched = digest_of(&partial)?;
    if fetched != digest {
        let _ = std::fs::remove_file(&partial);
        return Err(io(
            &url,
            format!("the download's SHA-256 is {fetched}, not the published {digest}; refused"),
        ));
    }
    make_executable(&partial)?;
    let binary = dir.join("tailwindcss");
    std::fs::rename(&partial, &binary).map_err(|e| io(&binary.display().to_string(), e.to_string()))
}

/// This process's own download file, so two processes fetching at once never share one.
fn partial_in(dir: &Path) -> PathBuf {
    dir.join(format!(".tailwindcss.{}.download", std::process::id()))
}

fn digest_of(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).map_err(|e| io(&path.display().to_string(), e.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| io(&path.display().to_string(), e.to_string()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn io(path: &str, reason: String) -> BuildError {
    BuildError::Tailwind {
        path: path.to_owned(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_supported_platform_has_its_pinned_digest() {
        for (os, arch, expected) in [
            ("linux", "x86_64", "linux-x64"),
            ("linux", "aarch64", "linux-arm64"),
            ("macos", "x86_64", "macos-x64"),
            ("macos", "aarch64", "macos-arm64"),
        ] {
            let platform = platform(os, arch).unwrap();
            assert_eq!(platform, expected);
            assert_eq!(digest_for(&platform).unwrap().len(), 64);
        }
        assert!(platform("windows", "x86_64").is_err());
        assert!(platform("linux", "riscv64").is_err());
    }

    #[test]
    fn each_process_downloads_into_its_own_file() {
        let dir = Path::new("/cache/tailwindcss/4.1.11");
        let partial = partial_in(dir);
        assert_eq!(partial.parent(), Some(dir));
        assert!(
            partial
                .to_string_lossy()
                .ends_with(&format!(".tailwindcss.{}.download", std::process::id())),
            "two processes fetching at once never write the same file: {}",
            partial.display()
        );
    }

    #[test]
    fn a_binary_is_known_only_by_its_sha256() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tailwindcss");
        std::fs::write(&file, b"abc").unwrap();
        assert_eq!(
            digest_of(&file).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_ne!(digest_of(&file).unwrap(), digest_for("linux-x64").unwrap());
    }

    fn tailwind_in(dir: &Path, bytes: &[u8]) -> PathBuf {
        let file = dir.join("tailwindcss");
        std::fs::write(&file, bytes).unwrap();
        file
    }

    #[test]
    fn an_installed_binary_with_the_pinned_digest_is_used_without_a_download() {
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let wrong = tempfile::tempdir().unwrap();
        tailwind_in(wrong.path(), b"other");
        let right = tempfile::tempdir().unwrap();
        tailwind_in(right.path(), b"abc");
        let empty = tempfile::tempdir().unwrap();
        let path = std::env::join_paths([empty.path(), wrong.path(), right.path()]).unwrap();

        // On PATH: the first tailwindcss whose digest is the pinned one.
        assert_eq!(
            installed_dir(None, Some(&path), abc),
            Some(right.path().to_path_buf())
        );
        // TAILWINDCSS_BIN wins over PATH when its digest matches.
        let named = tempfile::tempdir().unwrap();
        let bin = tailwind_in(named.path(), b"abc");
        assert_eq!(
            installed_dir(Some(bin.as_os_str()), Some(&path), abc),
            Some(named.path().to_path_buf())
        );
        // A TAILWINDCSS_BIN with another digest is passed over for PATH.
        let other = tailwind_in(wrong.path(), b"other");
        assert_eq!(
            installed_dir(Some(other.as_os_str()), Some(&path), abc),
            Some(right.path().to_path_buf())
        );
        // Nothing matching: the caller falls back to the download.
        let only_wrong = std::env::join_paths([wrong.path()]).unwrap();
        assert_eq!(installed_dir(None, Some(&only_wrong), abc), None);
        assert_eq!(installed_dir(None, None, abc), None);
    }

    /// Fetches the real release once; run with `--run-ignored only` where the network is up.
    #[test]
    #[ignore = "downloads the Tailwind release"]
    fn the_pinned_release_is_fetched_and_verified() {
        let dir = tempfile::tempdir().unwrap();
        let platform = platform(std::env::consts::OS, std::env::consts::ARCH).unwrap();
        let digest = digest_for(&platform).unwrap();
        fetch(&platform, digest, dir.path()).unwrap();
        let binary = dir.path().join("tailwindcss");
        assert_eq!(digest_of(&binary).unwrap(), digest);
        let version = Command::new(&binary).arg("--help").output().unwrap();
        assert!(String::from_utf8_lossy(&version.stdout).contains(VERSION));
    }
}
