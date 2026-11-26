use std::env;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{ErrorKind, Read, Seek, Write};
use std::process;
use std::thread;
use std::time::{Duration, Instant};

use fundamental_core::fs::replace_file;
use tracing::{debug, info, instrument, warn};

use crate::{LocalError, LocalState, Registry};

/// The registry on disk, changed only under an exclusive lock on a file that is never replaced.
///
/// Each change is written temp, fsync, rename, fsync(dir) before the lock is released.
#[derive(Debug, Clone)]
pub struct RegistryStore {
    state: LocalState,
    patience: Duration,
}

/// How long a change waits for another to finish before refusing.
const PATIENCE: Duration = Duration::from_secs(30);
/// How often a waiting change asks again.
const POLL: Duration = Duration::from_millis(25);

impl RegistryStore {
    /// The registry kept under `state`.
    #[must_use]
    pub const fn new(state: LocalState) -> Self {
        Self {
            state,
            patience: PATIENCE,
        }
    }

    /// The same store, waiting at most `patience` for the lock.
    #[must_use]
    pub const fn patience(mut self, patience: Duration) -> Self {
        self.patience = patience;
        self
    }

    /// The state directory the registry lives in.
    #[must_use]
    pub const fn state(&self) -> &LocalState {
        &self.state
    }

    /// The registry as it stands, read under the lock.
    ///
    /// # Errors
    ///
    /// As [`transact`](Self::transact).
    pub fn read(&self) -> Result<Registry, LocalError> {
        self.transact(|registry| Ok(registry.clone()))
    }

    /// Runs `change` on the registry under the exclusive lock and writes the result back when it
    /// changed; an error writes nothing.
    ///
    /// # Errors
    ///
    /// [`LocalError::LockTimeout`] (`LOCAL_020`), [`LocalError::RegistryCorrupt`] (`LOCAL_011`),
    /// [`LocalError::Io`], or whatever `change` returns.
    #[instrument(skip_all, fields(registry = %self.state.registry().display()))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn transact<T>(
        &self,
        change: impl FnOnce(&mut Registry) -> Result<T, LocalError>,
    ) -> Result<T, LocalError> {
        self.state.private_dir(self.state.dir())?;
        let lock = self.acquire()?;
        let path = self.state.registry();
        let before = match fs::read_to_string(&path) {
            Ok(text) => Registry::parse(&text, &path)?,
            Err(error) if error.kind() == ErrorKind::NotFound => Registry::default(),
            Err(error) => return Err(LocalError::io(&path, "read", error)),
        };
        let mut after = before.clone();
        let answer = change(&mut after)?;
        if after != before {
            let text = after.to_toml(&path)?;
            replace_file(&path, text.as_bytes())
                .map_err(|error| LocalError::io(&path, "written", error))?;
            debug!("local.registry_written");
        }
        drop(lock);
        Ok(answer)
    }

    #[expect(
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    fn acquire(&self) -> Result<File, LocalError> {
        let path = self.state.lock();
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|error| LocalError::io(&path, "opened", error))?;
        let started = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) if started.elapsed() < self.patience => {
                    thread::sleep(POLL);
                }
                Err(TryLockError::WouldBlock) => {
                    let mut holder = String::new();
                    if let Err(error) = file.read_to_string(&mut holder) {
                        warn!(%error, "the lock's holder could not be read");
                    }
                    let waited_ms =
                        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                    warn!(waited_ms, holder = %holder.trim(), "local.lock: refused after waiting");
                    return Err(LocalError::LockTimeout {
                        waited_ms,
                        holder: if holder.trim().is_empty() {
                            "holder unknown".to_owned()
                        } else {
                            holder.trim().to_owned()
                        },
                    });
                }
                Err(TryLockError::Error(error)) => {
                    return Err(LocalError::io(&path, "locked", error));
                }
            }
        }
        let waited_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        if waited_ms > 0 {
            info!(waited_ms, "local.lock");
        }
        let holder = format!(
            "pid {} ({})",
            process::id(),
            env::args().collect::<Vec<_>>().join(" ")
        );
        let recorded = file
            .set_len(0)
            .and_then(|()| file.rewind())
            .and_then(|()| file.write_all(holder.as_bytes()));
        if let Err(error) = recorded {
            warn!(%error, "the lock's holder could not be recorded");
        }
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    use fundamental_core::Error as _;

    use super::RegistryStore;
    use fundamental_core::address::TestApex;

    use crate::{ApexSet, InstanceKey, InstanceLabel, InstanceRole, LocalState, ProjectName};

    fn apexes() -> ApexSet {
        ApexSet::new(TestApex::parse("p.test")).expect("one apex")
    }

    fn claim(store: &RegistryStore, label: &str) -> String {
        let key = InstanceKey::new(
            ProjectName::parse("p").expect("a name"),
            InstanceRole::Lane(InstanceLabel::parse(label).expect("a label")),
        );
        store
            .transact(|registry| {
                registry.register_project(key.project(), &apexes())?;
                registry.claim_instance(&key, &PathBuf::from(format!("/{label}")), |_| true)
            })
            .expect("claimed")
            .to_string()
    }

    #[test]
    fn concurrent_claims_get_distinct_addresses() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let state = LocalState::at(dir.path().join("local"));
        let barrier = Arc::new(Barrier::new(16));
        let handles: Vec<_> = (0..16)
            .map(|at| {
                let store = RegistryStore::new(state.clone());
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    claim(&store, &format!("t{at}"))
                })
            })
            .collect();
        let addresses: BTreeSet<String> = handles
            .into_iter()
            .map(|handle| handle.join().expect("no panic"))
            .collect();
        assert_eq!(addresses.len(), 16);
        let registry = RegistryStore::new(state).read().expect("readable");
        assert_eq!(registry.instances().count(), 16);
    }

    #[test]
    fn a_held_lock_is_refused_after_the_stores_patience() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let state = LocalState::at(dir.path().join("local"));
        let holder = RegistryStore::new(state.clone());
        let waiter = RegistryStore::new(state).patience(Duration::from_millis(50));
        let refused = holder
            .transact(|_| {
                Ok(waiter
                    .read()
                    .map(|_| ())
                    .map_err(|error| error.error_code()))
            })
            .expect("the holder's own transaction");
        assert_eq!(refused, Err("LOCAL_020"));
    }

    #[test]
    fn a_refused_change_writes_nothing() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let state = LocalState::at(dir.path().join("local"));
        let store = RegistryStore::new(state.clone());
        let _ = store.transact::<()>(|registry| {
            registry.release(&InstanceKey::new(
                ProjectName::parse("p").expect("a name"),
                InstanceRole::Main,
            ));
            Err(crate::LocalError::InstanceNotUp)
        });
        assert!(!state.registry().exists());
    }
}
