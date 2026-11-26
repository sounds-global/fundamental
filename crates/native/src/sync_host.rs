use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use crate::sync_host_config::SyncHostConfig;
use fundamental_offline::db::{OfflineConfig, OfflineDatabase};
use fundamental_sync::wire::HostedRows;
use fundamental_ws_client::{SyncClient, SyncClientConfig};
use futures::StreamExt;
use serde_json::Value;
use tokio::runtime::Builder as RuntimeBuilder;
use tokio::sync::{mpsc, oneshot};
use tokio::task::{LocalSet, spawn_local};
use tracing::{debug, error, info, instrument};

use crate::NativeError;
use crate::device_slice_registry::DeviceSliceRegistry;

/// How a table change is announced; the builder emits it as `fundamental:live:{table}`.
pub(crate) type ChangeSink = Arc<dyn Fn(&'static str) + Send + Sync>;

/// The hosted database: one dedicated thread owning the durable SQLite mirror
/// and every [`SyncClient`] session, syncing whether or not a webview is awake.
///
/// The database is `!Sync`, so a dedicated current-thread runtime with a [`LocalSet`] owns it,
/// reached over a command channel. Parameter-less slices sync eagerly and are never released;
/// the thread stops when the last clone drops.
#[derive(Clone, Debug)]
pub struct SyncHost {
    commands: mpsc::Sender<HostCommand>,
}

pub(crate) struct SyncHostState(pub(crate) SyncHost);

enum HostCommand {
    Bind {
        slice: String,
        params: Value,
        reply: oneshot::Sender<Result<(), NativeError>>,
    },
    Unbind {
        slice: String,
        params: Value,
        reply: oneshot::Sender<Result<(), NativeError>>,
    },
    Rows {
        slice: String,
        reply: oneshot::Sender<Result<HostedRows, NativeError>>,
    },
}

impl std::fmt::Debug for HostCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (name, slice) = match self {
            Self::Bind { slice, .. } => ("Bind", slice),
            Self::Unbind { slice, .. } => ("Unbind", slice),
            Self::Rows { slice, .. } => ("Rows", slice),
        };
        f.debug_struct(name).field("slice", slice).finish()
    }
}

impl SyncHost {
    /// Starts the host thread: opens the database at `offline`, creates every mirror table,
    /// begins the eager sessions and serves commands; `on_change` fires with each changed table.
    ///
    /// # Errors
    /// [`NativeError::SyncHostStart`] when the runtime or thread cannot be built. A database
    /// that fails to open is logged once and every command answers
    /// [`NativeError::SyncHostUnavailable`].
    pub fn start(
        offline: OfflineConfig,
        config: SyncHostConfig,
        registry: DeviceSliceRegistry,
        on_change: impl Fn(&'static str) + Send + Sync + 'static,
    ) -> Result<Self, NativeError> {
        let runtime = RuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|source| NativeError::SyncHostStart { source })?;
        let (commands, inbox) = mpsc::channel(64);
        let sink: ChangeSink = Arc::new(on_change);

        std::thread::Builder::new()
            .name("fundamental-sync-host".to_owned())
            .spawn(move || {
                let local = LocalSet::new();
                runtime.block_on(local.run_until(serve(inbox, offline, config, registry, sink)));
                info!("sync host thread stopped");
            })
            .map_err(|source| NativeError::SyncHostStart { source })?;

        Ok(Self { commands })
    }

    /// Starts (or joins) the sync session for `slice` with `params`.
    ///
    /// # Errors
    /// [`NativeError::UnknownSlice`] / [`NativeError::SliceParams`] for a bad
    /// request; [`NativeError::SyncHostDatabase`] when the database is down;
    /// [`NativeError::SyncHostUnavailable`] when the host thread is gone.
    pub async fn bind(&self, slice: impl Into<String>, params: Value) -> Result<(), NativeError> {
        let (reply, answer) = oneshot::channel();
        self.send(HostCommand::Bind {
            slice: slice.into(),
            params,
            reply,
        })
        .await?;
        answer.await.map_err(|_| NativeError::SyncHostUnavailable)?
    }

    /// Releases one binding of `slice` with `params`; the session stops at its last binding,
    /// unless eager.
    ///
    /// # Errors
    /// As [`bind`](Self::bind); releasing a missing binding is not an error.
    pub async fn unbind(&self, slice: impl Into<String>, params: Value) -> Result<(), NativeError> {
        let (reply, answer) = oneshot::channel();
        self.send(HostCommand::Unbind {
            slice: slice.into(),
            params,
            reply,
        })
        .await?;
        answer.await.map_err(|_| NativeError::SyncHostUnavailable)?
    }

    /// Reads `slice`'s current local rows.
    ///
    /// # Errors
    /// [`NativeError::UnknownSlice`] for an unregistered slice;
    /// [`NativeError::SyncHostDatabase`] when the read fails;
    /// [`NativeError::SyncHostUnavailable`] when the host thread is gone.
    pub async fn rows(&self, slice: impl Into<String>) -> Result<HostedRows, NativeError> {
        let (reply, answer) = oneshot::channel();
        self.send(HostCommand::Rows {
            slice: slice.into(),
            reply,
        })
        .await?;
        answer.await.map_err(|_| NativeError::SyncHostUnavailable)?
    }

    async fn send(&self, command: HostCommand) -> Result<(), NativeError> {
        self.commands
            .send(command)
            .await
            .map_err(|_| NativeError::SyncHostUnavailable)
    }
}

struct Session {
    client: SyncClient,
    refs: usize,
    pinned: bool,
}

#[instrument(name = "sync_host", skip_all)]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn serve(
    mut inbox: mpsc::Receiver<HostCommand>,
    offline: OfflineConfig,
    config: SyncHostConfig,
    registry: DeviceSliceRegistry,
    on_change: ChangeSink,
) {
    let sync = match config.sync_config().await {
        Ok(sync) => sync,
        Err(error) => {
            error!(%error, "sync host token resolution failed; live data is down");
            while let Some(command) = inbox.recv().await {
                answer_unavailable(command);
            }
            return;
        }
    };

    let db = match OfflineDatabase::open(offline).await {
        Ok(db) => Rc::new(db),
        Err(error) => {
            // Logged once here; every command then answers "unavailable".
            error!(%error, "hosted offline database failed to open; live data is down");
            while let Some(command) = inbox.recv().await {
                answer_unavailable(command);
            }
            return;
        }
    };

    // Every mirror table up front, so a read never races the first session.
    let mut tables = BTreeSet::new();
    for entry in registry.iter() {
        if let Err(error) = entry.ensure_table(&db) {
            error!(%error, slice = entry.slice(), "mirror table setup failed");
        }
        tables.insert(entry.table());
    }
    for table in tables {
        let mut changes = db.watch(table);
        let sink = Arc::clone(&on_change);
        spawn_local(async move {
            while changes.next().await.is_some() {
                sink(table);
            }
        });
    }

    let mut sessions: HashMap<String, Session> = HashMap::new();

    for entry in registry.iter() {
        if let Some(binding) = entry.eager_binding() {
            let key = session_key(entry.slice(), binding.params());
            let client = entry.start_session(sync.clone(), binding, Rc::clone(&db));
            info!(slice = entry.slice(), "eager sync session started");
            sessions.insert(
                key,
                Session {
                    client,
                    refs: 0,
                    pinned: true,
                },
            );
        }
    }

    info!(
        slices = registry.len(),
        eager = sessions.len(),
        "sync host serving"
    );

    while let Some(command) = inbox.recv().await {
        debug!(?command, "sync host command");
        match command {
            HostCommand::Bind {
                slice,
                params,
                reply,
            } => {
                let result = handle_bind(&registry, &sync, &db, &mut sessions, &slice, params);
                let _ = reply.send(result);
            }
            HostCommand::Unbind {
                slice,
                params,
                reply,
            } => {
                let result = handle_unbind(&registry, &mut sessions, &slice, params);
                let _ = reply.send(result);
            }
            HostCommand::Rows { slice, reply } => {
                let result = registry.get(&slice).and_then(|entry| entry.rows(&db));
                let _ = reply.send(result);
            }
        }
    }
    drop(sessions);
}

fn answer_unavailable(command: HostCommand) {
    match command {
        HostCommand::Bind { reply, .. } | HostCommand::Unbind { reply, .. } => {
            let _ = reply.send(Err(NativeError::SyncHostUnavailable));
        }
        HostCommand::Rows { reply, .. } => {
            let _ = reply.send(Err(NativeError::SyncHostUnavailable));
        }
    }
}

#[expect(clippy::too_many_arguments, reason = "readability baseline")]
fn handle_bind(
    registry: &DeviceSliceRegistry,
    sync: &SyncClientConfig,
    db: &Rc<OfflineDatabase>,
    sessions: &mut HashMap<String, Session>,
    slice: &str,
    params: Value,
) -> Result<(), NativeError> {
    let entry = registry.get(slice)?;
    let binding = entry.binding(params)?;
    let key = session_key(slice, binding.params());
    if let Some(session) = sessions.get_mut(&key) {
        session.refs += 1;
        debug!(slice, refs = session.refs, "joined existing sync session");
        return Ok(());
    }
    let client = entry.start_session(sync.clone(), binding, Rc::clone(db));
    info!(slice, "sync session started");
    sessions.insert(
        key,
        Session {
            client,
            refs: 1,
            pinned: false,
        },
    );
    Ok(())
}

#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn handle_unbind(
    registry: &DeviceSliceRegistry,
    sessions: &mut HashMap<String, Session>,
    slice: &str,
    params: Value,
) -> Result<(), NativeError> {
    let entry = registry.get(slice)?;
    let binding = entry.binding(params)?;
    let key = session_key(slice, binding.params());
    let Some(session) = sessions.get_mut(&key) else {
        debug!(slice, "unbind for a session that is not running");
        return Ok(());
    };
    session.refs = session.refs.saturating_sub(1);
    if session.refs == 0 && !session.pinned {
        info!(slice, "sync session stopped (last binding released)");
        if let Some(session) = sessions.remove(&key) {
            drop(session.client);
        }
    }
    Ok(())
}

fn session_key(slice: &str, params: &Value) -> String {
    format!("{slice}\u{1f}{params}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    fn host_config() -> SyncHostConfig {
        SyncHostConfig::new("ws://127.0.0.1:0")
    }

    #[tokio::test]
    async fn an_unknown_slice_is_a_typed_404_through_the_host() {
        let host = SyncHost::start(
            OfflineConfig::memory(),
            host_config(),
            DeviceSliceRegistry::new(),
            |_| {},
        )
        .expect("host starts");

        let err = host.rows("nope").await.expect_err("unknown slice");
        assert_eq!(err.error_code(), "NATIVE_016");

        let err = host
            .bind("nope", serde_json::json!({}))
            .await
            .expect_err("unknown slice");
        assert_eq!(err.error_code(), "NATIVE_016");
    }

    #[tokio::test]
    async fn a_gone_host_thread_answers_retryable_unavailable() {
        // A host whose thread is gone answers the typed, retryable 503.
        let (commands, inbox) = mpsc::channel(1);
        drop(inbox);
        let host = SyncHost { commands };

        let err = host.rows("x").await.expect_err("closed channel");
        assert_eq!(err.error_code(), "NATIVE_015");
        assert_eq!(err.http_status_code(), 503);
        assert!(err.is_retryable());
    }

    #[tokio::test]
    async fn a_failed_database_open_answers_unavailable_but_the_host_survives() {
        // The parent is a file, so SQLite cannot create the database; the host still starts.
        let dir = std::env::temp_dir().join("fundamental-sync-host-test-file");
        std::fs::write(&dir, b"not a directory").expect("write blocker file");
        let offline = fundamental_offline::db::OfflineConfig {
            location: fundamental_offline::db::OfflineLocation::Persistent(
                dir.join("db.sqlite").to_string_lossy().into_owned(),
            ),
            ..OfflineConfig::memory()
        };

        let host = SyncHost::start(offline, host_config(), DeviceSliceRegistry::new(), |_| {})
            .expect("host starts even when the database cannot open");
        let err = host.rows("x").await.expect_err("database is down");
        assert_eq!(err.error_code(), "NATIVE_015");
    }
}
