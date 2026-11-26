//! [`locked_bindgen_cli`] — the `wasm-bindgen` CLI at the version a workspace's lockfile binds.

use std::path::{Path, PathBuf};

use crate::BuildError;
use crate::wasm::bindgen_cli_version;

/// The `wasm-bindgen` CLI at the version `lockfile` binds: the one `scripts/wasm-test.sh`
/// installs under the cache, else the one first on `PATH` when it is that version.
///
/// # Errors
/// [`BuildError::Io`] when `lockfile` cannot be read, and [`BuildError::BindgenUnlocked`] when it
/// names no `wasm-bindgen` or no CLI at its version is installed.
pub fn locked_bindgen_cli(lockfile: &Path) -> Result<PathBuf, BuildError> {
    let lock = std::fs::read_to_string(lockfile).map_err(|source| BuildError::Io {
        path: lockfile.to_path_buf(),
        source,
    })?;
    let locked = locked_version(&lock).ok_or_else(|| BuildError::BindgenUnlocked {
        locked: "none".to_owned(),
        installed: format!("{} names no wasm-bindgen", lockfile.display()),
    })?;
    let cached = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .map(|cache| cached_cli(&cache, locked))
        .filter(|cli| cli.is_file());
    if let Some(cli) = cached {
        return Ok(cli);
    }
    let installed = bindgen_cli_version().unwrap_or_default();
    if speaks(&installed, locked) {
        return Ok(PathBuf::from("wasm-bindgen"));
    }
    Err(BuildError::BindgenUnlocked {
        locked: locked.to_owned(),
        installed: if installed.is_empty() {
            "none on PATH".to_owned()
        } else {
            installed
        },
    })
}

fn locked_version(lock: &str) -> Option<&str> {
    lock.split("[[package]]")
        .find(|package| {
            package
                .lines()
                .any(|line| line == "name = \"wasm-bindgen\"")
        })?
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .map(|version| version.trim_end_matches('"'))
}

fn cached_cli(root: &Path, version: &str) -> PathBuf {
    root.join(format!(
        "fundamental/wasm-bindgen-cli/{version}/bin/wasm-bindgen"
    ))
}

fn speaks(installed: &str, version: &str) -> bool {
    installed.rsplit(' ').next() == Some(version)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const LOCK: &str = r#"
[[package]]
name = "wasm-bindgen-macro"
version = "0.2.999"

[[package]]
name = "wasm-bindgen"
version = "0.2.122"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

    #[test]
    fn the_lockfile_names_the_version_the_bundle_is_bound_with() {
        assert_eq!(locked_version(LOCK), Some("0.2.122"));
    }

    #[test]
    fn a_lockfile_without_wasm_bindgen_names_none() {
        let without = "[[package]]\nname = \"wasm-bindgen-macro\"\nversion = \"0.2.122\"\n";
        assert_eq!(locked_version(without), None);
        assert_eq!(locked_version(""), None);
    }

    #[test]
    fn the_cached_cli_sits_under_its_version() {
        assert_eq!(
            cached_cli(Path::new("/home/me/.cache"), "0.2.122"),
            Path::new("/home/me/.cache/fundamental/wasm-bindgen-cli/0.2.122/bin/wasm-bindgen")
        );
    }

    #[test]
    fn the_cli_on_the_path_serves_only_at_the_locked_version() {
        assert!(speaks("wasm-bindgen 0.2.122", "0.2.122"));
        assert!(!speaks("wasm-bindgen 0.2.130", "0.2.122"));
        assert!(!speaks("wasm-bindgen 10.2.122", "0.2.122"));
        assert!(!speaks("", "0.2.122"));
    }
}
