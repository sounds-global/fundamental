//! An object store on the local file system, for a single host.

use crate::{ByteRange, ByteStream, ObjectBody, ObjectStore, StoreError};
use bytes::{Bytes, BytesMut};
use fundamental_core::ProviderName;
use futures::{StreamExt, stream};
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tracing::{error, instrument, warn};

/// An [`ObjectStore`] that keeps each object as a file under one directory, for a single host.
///
/// A put is renamed into place only once whole; keys are `/`-separated `[a-z0-9._-]` segments,
/// never `.` or `..`; directories are `0700` and files `0600`. A signed read is a `file://` path.
#[derive(Clone, Debug)]
pub struct FsObjectStore {
    root: PathBuf,
}

const PROVIDER: ProviderName = ProviderName::new("fs");
const STAGING: &str = ".staging";
const CHUNK: usize = 64 * 1024;

impl FsObjectStore {
    /// The store under `root`, an existing absolute directory.
    ///
    /// # Errors
    /// [`StoreError::InvalidSettings`] when `root` is relative or not a directory.
    pub async fn open(root: impl AsRef<Path>) -> Result<Self, StoreError> {
        let root = root.as_ref();
        if !root.is_absolute() {
            return Err(StoreError::InvalidSettings {
                problems: format!("the store's root {} is not absolute", root.display()),
            });
        }
        let is_dir = fs::metadata(root)
            .await
            .is_ok_and(|metadata| metadata.is_dir());
        if !is_dir {
            return Err(StoreError::InvalidSettings {
                problems: format!("the store's root {} is not a directory", root.display()),
            });
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// The file an object under `key` is kept in.
    ///
    /// # Errors
    /// [`StoreError::Refused`] (400) for a key outside the store's alphabet.
    pub fn path_of(&self, key: &str) -> Result<PathBuf, StoreError> {
        let fits = !key.is_empty()
            && key.split('/').all(|segment| {
                !segment.is_empty()
                    && segment != "."
                    && segment != ".."
                    && segment != STAGING
                    && segment.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || matches!(byte, b'.' | b'_' | b'-')
                    })
            });
        if !fits {
            warn!(
                key_len = key.len(),
                "a key outside the store's alphabet was refused"
            );
            return Err(StoreError::Refused {
                provider: PROVIDER,
                status: 400,
            });
        }
        Ok(self.root.join(key))
    }

    async fn staged(&self, max_len: u64, body: ByteStream) -> Result<(PathBuf, u64), StoreError> {
        let staging = self.root.join(STAGING);
        make_dirs(&staging).await?;
        let path = staging.join(staging_name()?);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .await
            .map_err(|cause| io_failed("open a staging file", &cause))?;
        let outcome = write_bounded(&mut file, max_len, body).await;
        match outcome {
            Ok(written) => Ok((path, written)),
            Err(refused) => {
                discard(&path).await;
                Err(refused)
            }
        }
    }
}

impl ObjectStore for FsObjectStore {
    fn provider(&self) -> ProviderName {
        PROVIDER
    }

    #[instrument(skip(self, body), fields(provider = PROVIDER.as_str()))]
    async fn put(&self, key: &str, max_len: u64, body: ByteStream) -> Result<u64, StoreError> {
        let target = self.path_of(key)?;
        let (staged, len) = self.staged(max_len, body).await?;
        if let Some(parent) = target.parent()
            && let Err(refused) = make_dirs(parent).await
        {
            discard(&staged).await;
            return Err(refused);
        }
        if let Err(cause) = fs::rename(&staged, &target).await {
            discard(&staged).await;
            return Err(io_failed("move a staged object into place", &cause));
        }
        Ok(len)
    }

    #[instrument(skip(self), fields(provider = PROVIDER.as_str()))]
    async fn get(&self, key: &str, range: Option<ByteRange>) -> Result<ObjectBody, StoreError> {
        let path = self.path_of(key)?;
        let mut file = match OpenOptions::new()
            .read(true)
            // An object is never read through a link someone planted under the root.
            .custom_flags(rustix_o_nofollow())
            .open(&path)
            .await
        {
            Ok(file) => file,
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => {
                return Err(StoreError::NotFound);
            }
            Err(cause) => return Err(io_failed("open an object", &cause)),
        };
        let object_len = file
            .metadata()
            .await
            .map_err(|cause| io_failed("read an object's length", &cause))?
            .len();
        let (first, len) = match range {
            None => (0, object_len),
            Some(range) => {
                let range = range.within(object_len).ok_or(StoreError::Refused {
                    provider: PROVIDER,
                    status: 416,
                })?;
                (range.first(), range.last() - range.first() + 1)
            }
        };
        file.seek(io::SeekFrom::Start(first))
            .await
            .map_err(|cause| io_failed("seek in an object", &cause))?;
        Ok(ObjectBody {
            len,
            object_len,
            stream: read_stream(file, len),
        })
    }

    #[instrument(skip(self), fields(provider = PROVIDER.as_str()))]
    async fn head(&self, key: &str) -> Result<Option<u64>, StoreError> {
        let path = self.path_of(key)?;
        match fs::symlink_metadata(&path).await {
            Ok(metadata) if metadata.is_file() => Ok(Some(metadata.len())),
            Ok(_) => Ok(None),
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(cause) => Err(io_failed("read an object's length", &cause)),
        }
    }

    #[instrument(skip(self), fields(provider = PROVIDER.as_str()))]
    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        let path = self.path_of(key)?;
        match fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(cause) => Err(io_failed("delete an object", &cause)),
        }
    }

    fn presign_get(&self, key: &str, _ttl: Duration) -> Result<String, StoreError> {
        Ok(format!("file://{}", self.path_of(key)?.display()))
    }

    async fn deletes_are_final(&self) -> Result<(), StoreError> {
        Ok(())
    }
}

async fn write_bounded(
    file: &mut File,
    max_len: u64,
    mut body: ByteStream,
) -> Result<u64, StoreError> {
    let mut written = 0_u64;
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        written = written.saturating_add(chunk.len() as u64);
        if written > max_len {
            return Err(StoreError::TooLarge { max: max_len });
        }
        file.write_all(&chunk)
            .await
            .map_err(|cause| io_failed("write a staging file", &cause))?;
    }
    file.sync_all()
        .await
        .map_err(|cause| io_failed("sync a staging file", &cause))?;
    Ok(written)
}

fn read_stream(file: File, len: u64) -> ByteStream {
    Box::pin(stream::unfold((file, len), |(mut file, left)| async move {
        if left == 0 {
            return None;
        }
        let want = usize::try_from(left).map_or(CHUNK, |left| left.min(CHUNK));
        let mut buffer = BytesMut::zeroed(want);
        match file.read_exact(&mut buffer).await {
            Ok(_) => Some((Ok(Bytes::from(buffer)), (file, left - want as u64))),
            Err(cause) => Some((Err(io_failed("read an object", &cause)), (file, 0))),
        }
    }))
}

async fn make_dirs(path: &Path) -> Result<(), StoreError> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || builder.create(&path))
        .await
        .map_err(|cause| {
            error!(%cause, "creating a directory did not finish");
            StoreError::Unreachable { provider: PROVIDER }
        })?
        .map_err(|cause| io_failed("create a directory", &cause))
}

async fn discard(path: &Path) {
    if let Err(cause) = fs::remove_file(path).await
        && cause.kind() != io::ErrorKind::NotFound
    {
        warn!(%cause, path = %path.display(), "a staging file could not be removed");
    }
}

fn staging_name() -> Result<String, StoreError> {
    let mut bytes = [0_u8; 16];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes).map_err(|_| {
        error!("the system's random source failed");
        StoreError::Unreachable { provider: PROVIDER }
    })?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn io_failed(step: &'static str, cause: &io::Error) -> StoreError {
    error!(step, %cause, kind = ?cause.kind(), "the file system refused");
    StoreError::Unreachable { provider: PROVIDER }
}

fn rustix_o_nofollow() -> i32 {
    i32::try_from(rustix::fs::OFlags::NOFOLLOW.bits()).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_key_never_reaches_outside_the_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FsObjectStore::open(dir.path()).await.expect("store");
        for key in [
            "",
            "/abs",
            "a/../b",
            "..",
            "a//b",
            "a/./b",
            "A",
            ".staging/x",
            "a b",
        ] {
            let refused = store.path_of(key).expect_err(key);
            assert!(
                matches!(refused, StoreError::Refused { status: 400, .. }),
                "{key}: {refused:?}"
            );
        }
        assert_eq!(
            store.path_of("uploads/b1/c.tar.zst").expect("fits"),
            dir.path().join("uploads/b1/c.tar.zst")
        );
    }

    #[tokio::test]
    async fn a_relative_or_missing_root_is_refused() {
        for root in ["relative", "/nonexistent/fundamental/store"] {
            assert!(matches!(
                FsObjectStore::open(root).await,
                Err(StoreError::InvalidSettings { .. })
            ));
        }
    }

    #[tokio::test]
    async fn a_planted_link_is_not_followed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::NamedTempFile::new().expect("outside");
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).expect("symlink");
        let store = FsObjectStore::open(dir.path()).await.expect("store");
        assert!(store.get("link", None).await.is_err());
        assert_eq!(store.head("link").await.expect("head"), None);
    }
}
