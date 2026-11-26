use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use tracing::warn;

/// The databases this process has already swept, by URL, since a schema lives inside one database.
fn swept() -> &'static Mutex<HashSet<String>> {
    static SWEPT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SWEPT.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Whether this is the first claim this process makes in `database_url`, recording that it is;
/// a poisoned registry answers `false`.
pub(super) fn first_sweep_in(database_url: &str) -> bool {
    match swept().lock() {
        Ok(mut swept) => swept.insert(database_url.to_owned()),
        Err(_) => {
            warn!("schema sweep: the sweep registry is poisoned; this claim does not sweep");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_database_is_swept_once_per_process() {
        let url = "postgresql://yugabyte@127.0.0.1:1/first_sweep_in_test";
        assert!(first_sweep_in(url), "the first claim here sweeps");
        assert!(!first_sweep_in(url), "a second does not");
        assert!(
            first_sweep_in("postgresql://yugabyte@127.0.0.1:1/another_database"),
            "another database is swept on its own first claim"
        );
    }
}
