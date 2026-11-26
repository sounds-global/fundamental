//! Every reconnecting client draws its delay from the one `fundamental_core::retry::Backoff`,
//! checked by construction: no entropy source or exponent in either client's source.

use std::fs;
use std::path::{Path, PathBuf};

const CLIENTS: [&str; 2] = ["nats", "ws_client"];

/// What a hand-rolled schedule needs: randomness, or a geometric growth.
const OWN_SCHEDULE_MARKERS: [&str; 5] = ["getrandom", "powi(", ".pow(", "1u32 <<", "1 <<"];

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| panic!("the crate sits in crates/"))
}

fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
fn every_reconnect_uses_the_shared_backoff() {
    let crates = crates_dir();
    let mut offences = Vec::new();
    for client in CLIENTS {
        let manifest = crates.join(client).join("Cargo.toml");
        if read(&manifest).contains("getrandom") {
            offences.push(format!("{} depends on getrandom", manifest.display()));
        }
        let mut files = Vec::new();
        sources(&crates.join(client).join("src"), &mut files);
        assert!(!files.is_empty(), "{client} has no sources to check");
        for file in files {
            let text = read(&file);
            for marker in OWN_SCHEDULE_MARKERS {
                if text.contains(marker) {
                    offences.push(format!("{} contains `{marker}`", file.display()));
                }
            }
        }
    }
    assert!(
        offences.is_empty(),
        "a client draws a reconnect schedule of its own:\n{}",
        offences.join("\n")
    );

    let nats = read(&crates.join("nats/src/client/nats_client.rs"));
    assert!(
        nats.contains("reconnect.backoff()?") && nats.contains("backoff.draw("),
        "NATS's reconnect delay is drawn from the shared Backoff"
    );
    let ws = read(&crates.join("ws_client/src/reconnect_schedule.rs"));
    assert!(
        ws.contains("use fundamental_core::retry::{Backoff") && ws.contains("backoff.draw("),
        "the sync client's reconnect delay is drawn from the shared Backoff"
    );
}
