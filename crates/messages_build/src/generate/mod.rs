//! [`generate`]: a crate's messages built from its `build.rs`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use mf2_build::{Build, Config, Emit, Features, Level, Lint, Missing, Strip};

use crate::{Environment, Gap, Level as PolicyLevel, Policy, module, signatures};

/// The runtime the generated module names: the crate `messages!()` came from, through the
/// umbrella or directly.
const FACADE: &str = "crate::m::__messages";

/// The crate's catalogs, where the generated module stands them.
const CATALOGS: &str = "crate::m::CATALOGS";

/// Builds the messages of the crate whose `build.rs` calls it into `OUT_DIR`: a catalog per
/// language and the `messages.rs` that `fundamental_messages::messages!()` includes, a missing
/// translation judged by the project's [`Policy`].
///
/// # Panics
/// Cargo did not set `CARGO_MANIFEST_DIR` or `OUT_DIR`, as it does for every build script.
pub fn generate() {
    let root = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets the crate's directory"),
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets the output directory"));
    println!("cargo::rerun-if-env-changed=CI");
    for manifest in Policy::manifests(&root) {
        println!("cargo::rerun-if-changed={}", manifest.display());
    }
    let built = Policy::read(&root, Environment::detect())
        .map_err(|problem| vec![problem])
        .and_then(|policy| build(&root, &out, &policy));
    if let Err(problems) = built {
        for problem in problems {
            for line in problem.lines() {
                println!("cargo::error={line}");
            }
        }
        std::process::exit(1);
    }
}

/// The build of the messages at `root` into `out`, a missing translation judged by `policy`.
///
/// # Errors
/// Every problem, each naming its message or file.
pub fn build(root: &Path, out: &Path, policy: &Policy) -> Result<(), Vec<String>> {
    let config = config(policy);
    let source = config.source_locale.clone();
    refused_calls(&root.join("locales"))?;
    let outcome = Build::at(root, out)
        .config(config)
        .features(Features::from_names([
            "host-std",
            "host-std-number-builtin",
        ]))
        .emit(Emit::Catalogs)
        .emit_cargo(true)
        .run()
        .and_then(mf2_build::Outcome::into_result)
        .map_err(|error| vec![error.to_string()])?;
    let mut files = String::new();
    for catalog in &outcome.catalogs {
        let tag = &catalog.tag;
        write(&out.join(format!("{tag}.catalog")), &catalog.bytes)?;
        write(&out.join(format!("{tag}.server")), &catalog.server)?;
        let _ = write!(
            files,
            "(\"{tag}\", include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{tag}.catalog\")), include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{tag}.server\"))), "
        );
    }
    let mut found = Vec::new();
    let dir = root.join("locales").join(&source);
    let mut sources: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|error| vec![format!("{}: {error}", dir.display())])?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "mf2"))
        .collect();
    sources.sort();
    for path in sources {
        let text = std::fs::read_to_string(&path)
            .map_err(|error| vec![format!("{}: {error}", path.display())])?;
        found.extend(signatures(&text).map_err(|problems| {
            problems
                .into_iter()
                .map(|problem| format!("{}: {problem}", path.display()))
                .collect::<Vec<_>>()
        })?);
    }
    let chains: Vec<String> = policy
        .fallback
        .iter()
        .map(|(language, chain)| {
            let chain: Vec<String> = chain.iter().map(|tag| format!("\"{tag}\"")).collect();
            format!("(\"{language}\", &[{}])", chain.join(", "))
        })
        .collect();
    let code = format!(
        "/// This crate's compiled catalogs, which a translator says its messages from by id.\npub static CATALOGS: {FACADE}::Catalogs = {FACADE}::Catalogs::new(\"{source}\", {:#x}, &[{files}]).falling_back(&[{}]);\n{}",
        outcome.manifest_hash,
        chains.join(", "),
        module(&found, FACADE, CATALOGS),
    );
    write(&out.join("messages.rs"), code.as_bytes())
}

/// The engine's configuration for `policy`.
fn config(policy: &Policy) -> Config {
    let mut config = Config::default();
    config.lints.insert(
        Lint::MissingTranslation,
        match policy.missing {
            PolicyLevel::Allow => Level::Allow,
            PolicyLevel::Warn => Level::Warn,
            PolicyLevel::Error => Level::Error,
        },
    );
    config.catalog.missing = match policy.gap {
        Gap::Borrowed => Missing::Fallback,
        Gap::Id => Missing::Id,
        Gap::Nothing => Missing::Empty,
    };
    config.fallback.clone_from(&policy.fallback);
    // The server finds a message by its id, so the ids stay in the catalog.
    config.catalog.strip = [Strip::Cold].into_iter().collect();
    // Every crate formats numbers the same way; one whose messages take none links it anyway.
    config.lints.insert(Lint::UnusedFeature, Level::Allow);
    config
        .functions
        .insert("brand".to_owned(), "fundamental_messages::BRAND".to_owned());
    config
}

/// Every call under `locales` the runtime cannot make, found before the engine compiles.
fn refused_calls(locales: &Path) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    let folders = std::fs::read_dir(locales)
        .map_err(|error| vec![format!("{}: {error}", locales.display())])?;
    for folder in folders.filter_map(Result::ok).map(|entry| entry.path()) {
        let Ok(files) = std::fs::read_dir(&folder) else {
            continue;
        };
        for path in files.filter_map(Result::ok).map(|entry| entry.path()) {
            if path.extension().is_none_or(|extension| extension != "mf2") {
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .map_err(|error| vec![format!("{}: {error}", path.display())])?;
            if let Err(found) = signatures(&text) {
                problems.extend(
                    found
                        .into_iter()
                        .map(|problem| format!("{}: {problem}", path.display())),
                );
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Vec<String>> {
    std::fs::write(path, bytes).map_err(|error| vec![format!("{}: {error}", path.display())])
}
