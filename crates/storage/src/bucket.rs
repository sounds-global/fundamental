//! A set of stores holding one kind of object.

use crate::{
    ByteRange, ByteStream, ObjectBody, ObjectKey, ObjectStore, Private, Public, PublicArtefact,
    ShardIndex, StoreError, StoredObject, Visibility,
};
use bytes::{Bytes, BytesMut};
use fundamental_encryption::{
    ContentKey, SealedStreamError, SegmentOpener, SegmentSealer, StreamLayout,
};
use futures::StreamExt;
use futures::stream;
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tracing::field::Empty;
use tracing::{Span, instrument};

const MAX_SHARDS: usize = 256;

/// One or more stores holding objects of kind `V`, written across them by key; a [`Private`]
/// bucket seals every object, a [`Public`] one stores [`PublicArtefact`]s as they are.
///
/// Plain bytes have no way into a private bucket:
///
/// ```compile_fail
/// # use fundamental_storage::{Bucket, MemoryObjectStore, ObjectKey, Private, PublicArtefact};
/// # async fn f(bucket: Bucket<Private, MemoryObjectStore>, key: ObjectKey<Private>, artefact: PublicArtefact) {
/// bucket.put(&key, artefact).await;
/// # }
/// ```
pub struct Bucket<V: Visibility, S: ObjectStore> {
    shards: Vec<S>,
    kind: PhantomData<fn() -> V>,
}

impl<V: Visibility, S: ObjectStore> Bucket<V, S> {
    /// A bucket over `shards`, at least one and at most 256.
    ///
    /// # Errors
    /// [`StoreError::InvalidSettings`] for no store or too many.
    pub fn new(shards: Vec<S>) -> Result<Self, StoreError> {
        if shards.is_empty() || shards.len() > MAX_SHARDS {
            return Err(StoreError::InvalidSettings {
                problems: format!(
                    "a bucket has 1 to {MAX_SHARDS} stores, not {}",
                    shards.len()
                ),
            });
        }
        Ok(Self {
            shards,
            kind: PhantomData,
        })
    }

    /// Refuses a bucket any of whose stores keeps what it deletes. Run at boot.
    ///
    /// # Errors
    /// The first store's refusal.
    pub async fn verify(&self) -> Result<(), StoreError> {
        for store in &self.shards {
            store.deletes_are_final().await?;
        }
        Ok(())
    }

    /// The store a new object under `key` is written to.
    #[must_use]
    pub fn shard_for(&self, key: &ObjectKey<V>) -> ShardIndex {
        let digest = blake3::hash(key.as_str().as_bytes());
        let spread = u64::from_le_bytes(digest.as_bytes()[..8].try_into().unwrap_or([0; 8]));
        // `shards.len()` is 1..=256, so the remainder fits in a `u8`.
        let index = spread % self.shards.len() as u64;
        ShardIndex::new(u8::try_from(index).unwrap_or(0))
    }

    /// The length of the object under `key` in `shard`, or `None` if there is none.
    ///
    /// # Errors
    /// The store's refusal, or [`StoreError::InvalidSettings`] for a shard this bucket lacks.
    pub async fn head(
        &self,
        shard: ShardIndex,
        key: &ObjectKey<V>,
    ) -> Result<Option<u64>, StoreError> {
        self.store(shard)?.head(key.as_str()).await
    }

    /// Deletes the object under `key` in `shard`; an absent object is already deleted.
    ///
    /// # Errors
    /// The store's refusal, or [`StoreError::InvalidSettings`] for a shard this bucket lacks.
    #[instrument(
        name = "storage.delete",
        skip_all,
        fields(visibility = V::NAME, shard = shard.get(), object_key = %key, code = Empty),
        err(Debug)
    )]
    pub async fn delete(&self, shard: ShardIndex, key: &ObjectKey<V>) -> Result<(), StoreError> {
        self.store(shard)?.delete(key.as_str()).await
    }

    fn store(&self, shard: ShardIndex) -> Result<&S, StoreError> {
        self.shards
            .get(usize::from(shard.get()))
            .ok_or_else(|| StoreError::InvalidSettings {
                problems: format!("no store at shard {}", shard.get()),
            })
    }
}

impl<S: ObjectStore> Bucket<Private, S> {
    /// Seals `content` under `content_key`, bound to `key` and `binding`, and stores it under `key`,
    /// refusing more than `max_len` bytes of content.
    ///
    /// # Errors
    /// [`StoreError::TooLarge`] past `max_len`, [`StoreError::BodyFailed`] if `content` fails,
    /// [`StoreError::Sealing`] if it cannot be sealed, or the store's refusal. Nothing is kept.
    #[instrument(
        name = "storage.put",
        skip_all,
        fields(
            visibility = "private",
            object_key = %key,
            shard = Empty,
            provider = Empty,
            content_len = Empty,
            stored_len = Empty,
        ),
        err(Debug)
    )]
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    pub async fn put_sealed(
        &self,
        key: &ObjectKey<Private>,
        content_key: &ContentKey,
        binding: &[&[u8]],
        max_len: u64,
        content: ByteStream,
    ) -> Result<StoredObject, StoreError> {
        let shard = self.shard_for(key);
        let store = self.store(shard)?;
        let span = Span::current();
        span.record("shard", shard.get());
        span.record("provider", store.provider().as_str());
        let max_stored =
            StreamLayout::ciphertext_len(max_len).ok_or(StoreError::TooLarge { max: max_len })?;
        let bound = bound_to(key, binding);
        let (sealer, header) = SegmentSealer::begin(content_key, &bound).map_err(sealing)?;
        let counted = Arc::new(AtomicU64::new(0));
        let sealed = sealed_stream(
            sealer,
            header.into_bytes(),
            content,
            max_len,
            Arc::clone(&counted),
        );
        let stored_len = store.put(key.as_str(), max_stored, sealed).await?;
        let content_len = counted.load(Ordering::Acquire);
        span.record("content_len", content_len);
        span.record("stored_len", stored_len);
        if StreamLayout::ciphertext_len(content_len) != Some(stored_len) {
            return Err(StoreError::BadReply {
                provider: store.provider(),
                detail: "stored a length other than the one sealed",
            });
        }
        Ok(StoredObject {
            shard,
            content_len,
            stored_len,
        })
    }

    /// Opens the object under `key` in `shard` and answers its content, or the bytes `range` names.
    ///
    /// # Errors
    /// [`StoreError::Tampered`] if the object is not what was sealed, [`StoreError::InvalidSettings`]
    /// for a range past the content, or the store's refusal.
    #[instrument(
        name = "storage.open",
        skip_all,
        fields(visibility = "private", shard = shard.get(), object_key = %key, ranged = range.is_some()),
        err(Debug)
    )]
    #[expect(
        clippy::too_many_lines,
        clippy::too_many_arguments,
        reason = "readability baseline"
    )]
    pub async fn open(
        &self,
        shard: ShardIndex,
        key: &ObjectKey<Private>,
        content_key: &ContentKey,
        binding: &[&[u8]],
        content_len: u64,
        range: Option<ByteRange>,
    ) -> Result<ObjectBody, StoreError> {
        let store = self.store(shard)?;
        let expected = StreamLayout::ciphertext_len(content_len)
            .ok_or(StoreError::Tampered { reason: "length" })?;
        let wanted = match range {
            Some(range) => {
                Some(
                    range
                        .within(content_len)
                        .ok_or_else(|| StoreError::InvalidSettings {
                            problems: "a range starts past the content".to_owned(),
                        })?,
                )
            }
            None => None,
        };
        let (first_byte, last_byte) = match wanted {
            Some(range) => (range.first(), range.last()),
            None => (0, content_len.saturating_sub(1)),
        };
        let first_segment = StreamLayout::segment_of(first_byte);
        let last_segment = if content_len == 0 {
            0
        } else {
            StreamLayout::segment_of(last_byte)
        };
        let (segments_start, _) = StreamLayout::segment_range(first_segment, content_len)
            .ok_or(StoreError::Tampered { reason: "length" })?;
        let (last_offset, last_len) = StreamLayout::segment_range(last_segment, content_len)
            .ok_or(StoreError::Tampered { reason: "length" })?;
        let header_len = StreamLayout::HEADER_LEN as u64;
        // The header comes with the segments when they start right after it.
        let read_from = if first_segment == 0 {
            0
        } else {
            segments_start
        };
        let read = ByteRange::new(read_from, last_offset + last_len - 1)
            .ok_or(StoreError::Tampered { reason: "length" })?;
        let mut body = store.get(key.as_str(), Some(read)).await?;
        if body.object_len != expected {
            return Err(StoreError::Tampered { reason: "length" });
        }
        let header = if first_segment == 0 {
            take_exact(&mut body.stream, header_len).await?
        } else {
            let mut head = store
                .get(key.as_str(), ByteRange::new(0, header_len - 1))
                .await?;
            take_exact(&mut head.stream, header_len).await?
        };
        let opener =
            SegmentOpener::begin(content_key, &bound_to(key, binding), &header).map_err(refused)?;
        let len = if content_len == 0 {
            0
        } else {
            last_byte - first_byte + 1
        };
        let plaintext = opened_stream(Opening {
            opener,
            sealed: body.stream,
            buffer: BytesMut::new(),
            next_segment: first_segment,
            last_segment,
            content_len,
            first_byte,
            last_byte,
        });
        Ok(ObjectBody {
            len,
            object_len: content_len,
            stream: plaintext,
        })
    }
}

impl<S: ObjectStore> Bucket<Public, S> {
    /// Stores `artefact` under `key` as it is.
    ///
    /// # Errors
    /// [`StoreError::TooLarge`], [`StoreError::BodyFailed`], or the store's refusal.
    #[instrument(
        name = "storage.put",
        skip_all,
        fields(visibility = "public", object_key = %key, shard = Empty, stored_len = Empty),
        err(Debug)
    )]
    pub async fn put(
        &self,
        key: &ObjectKey<Public>,
        artefact: PublicArtefact,
    ) -> Result<StoredObject, StoreError> {
        let shard = self.shard_for(key);
        Span::current().record("shard", shard.get());
        let stored_len = self
            .store(shard)?
            .put(key.as_str(), artefact.max_len, artefact.stream)
            .await?;
        Span::current().record("stored_len", stored_len);
        Ok(StoredObject {
            shard,
            content_len: stored_len,
            stored_len,
        })
    }

    /// Reads the artefact under `key`, or the bytes of it `range` names.
    ///
    /// # Errors
    /// [`StoreError::NotFound`], or the store's refusal.
    pub async fn get(
        &self,
        shard: ShardIndex,
        key: &ObjectKey<Public>,
        range: Option<ByteRange>,
    ) -> Result<ObjectBody, StoreError> {
        self.store(shard)?.get(key.as_str(), range).await
    }

    /// A URL anyone may fetch the artefact under `key` from, for `ttl`.
    ///
    /// # Errors
    /// [`StoreError::InvalidSettings`] if the store cannot sign one.
    pub fn presign_get(
        &self,
        shard: ShardIndex,
        key: &ObjectKey<Public>,
        ttl: Duration,
    ) -> Result<String, StoreError> {
        self.store(shard)?.presign_get(key.as_str(), ttl)
    }
}

struct Sealing {
    sealer: Option<SegmentSealer>,
    content: ByteStream,
    queue: VecDeque<Bytes>,
    max_len: u64,
    counted: Arc<AtomicU64>,
}

#[expect(clippy::excessive_nesting, reason = "readability baseline")]
fn sealed_stream(
    sealer: SegmentSealer,
    header: Vec<u8>,
    content: ByteStream,
    max_len: u64,
    counted: Arc<AtomicU64>,
) -> ByteStream {
    let state = Sealing {
        sealer: Some(sealer),
        content,
        queue: VecDeque::from([Bytes::from(header)]),
        max_len,
        counted,
    };
    Box::pin(stream::unfold(state, |mut state| async move {
        loop {
            if let Some(next) = state.queue.pop_front() {
                return Some((Ok(next), state));
            }
            state.sealer.as_ref()?;
            match state.content.next().await {
                Some(Ok(chunk)) => {
                    let total = state.counted.load(Ordering::Acquire) + chunk.len() as u64;
                    state.counted.store(total, Ordering::Release);
                    if total > state.max_len {
                        state.sealer = None;
                        return Some((Err(StoreError::TooLarge { max: state.max_len }), state));
                    }
                    let sealer = state.sealer.as_mut()?;
                    match sealer.update(&chunk) {
                        Ok(segments) => state
                            .queue
                            .extend(segments.into_iter().map(|s| Bytes::from(s.into_bytes()))),
                        Err(error) => {
                            state.sealer = None;
                            return Some((Err(sealing(error)), state));
                        }
                    }
                }
                Some(Err(error)) => {
                    state.sealer = None;
                    return Some((Err(error), state));
                }
                None => {
                    let sealer = state.sealer.take()?;
                    match sealer.finish() {
                        Ok(last) => state.queue.push_back(Bytes::from(last.into_bytes())),
                        Err(error) => return Some((Err(sealing(error)), state)),
                    }
                }
            }
        }
    }))
}

struct Opening {
    opener: SegmentOpener,
    sealed: ByteStream,
    buffer: BytesMut,
    next_segment: u64,
    last_segment: u64,
    content_len: u64,
    first_byte: u64,
    last_byte: u64,
}

fn opened_stream(state: Opening) -> ByteStream {
    Box::pin(stream::unfold(Some(state), |state| async move {
        let mut state = state?;
        if state.next_segment > state.last_segment {
            return None;
        }
        let index = state.next_segment;
        let Some((_, sealed_len)) = StreamLayout::segment_range(index, state.content_len) else {
            return Some((Err(StoreError::Tampered { reason: "length" }), None));
        };
        let sealed_len = usize::try_from(sealed_len).unwrap_or(usize::MAX);
        while state.buffer.len() < sealed_len {
            match state.sealed.next().await {
                Some(Ok(chunk)) => state.buffer.extend_from_slice(&chunk),
                Some(Err(error)) => return Some((Err(error), None)),
                None => return Some((Err(StoreError::Tampered { reason: "length" }), None)),
            }
        }
        let segment = state.buffer.split_to(sealed_len);
        let plain = match state.opener.open(index, state.content_len, &segment) {
            Ok(plain) => plain,
            Err(error) => return Some((Err(refused(error)), None)),
        };
        let segment_start = index * StreamLayout::SEGMENT_LEN as u64;
        let from = state.first_byte.saturating_sub(segment_start);
        let to = (state.last_byte + 1)
            .saturating_sub(segment_start)
            .min(plain.len() as u64);
        let from = usize::try_from(from).unwrap_or(0).min(plain.len());
        let to = usize::try_from(to).unwrap_or(0).max(from);
        let piece = Bytes::copy_from_slice(&plain[from..to]);
        state.next_segment += 1;
        if state.next_segment > state.last_segment && !state.buffer.is_empty() {
            return Some((Err(StoreError::Tampered { reason: "length" }), None));
        }
        Some((Ok(piece), Some(state)))
    }))
}

async fn take_exact(stream: &mut ByteStream, len: u64) -> Result<Vec<u8>, StoreError> {
    let len = usize::try_from(len).unwrap_or(usize::MAX);
    let mut taken = Vec::with_capacity(len);
    let mut rest: Option<Bytes> = None;
    while taken.len() < len {
        match stream.next().await {
            Some(Ok(chunk)) => {
                let need = len - taken.len();
                if chunk.len() > need {
                    taken.extend_from_slice(&chunk[..need]);
                    rest = Some(chunk.slice(need..));
                } else {
                    taken.extend_from_slice(&chunk);
                }
            }
            Some(Err(error)) => return Err(error),
            None => return Err(StoreError::Tampered { reason: "length" }),
        }
    }
    if let Some(rest) = rest {
        let remaining = std::mem::replace(stream, Box::pin(stream::empty()));
        *stream = Box::pin(stream::once(async move { Ok(rest) }).chain(remaining));
    }
    Ok(taken)
}

/// The object's own key, then the caller's binding, so a provider moving an object cannot make it read as another.
fn bound_to<'a>(key: &'a ObjectKey<Private>, binding: &[&'a [u8]]) -> Vec<&'a [u8]> {
    std::iter::once(key.as_str().as_bytes())
        .chain(binding.iter().copied())
        .collect()
}

fn sealing(error: SealedStreamError) -> StoreError {
    match error {
        SealedStreamError::TooLong => StoreError::TooLarge { max: u64::MAX },
        other => StoreError::Sealing {
            cause: other.into(),
        },
    }
}

fn refused(error: SealedStreamError) -> StoreError {
    match error {
        SealedStreamError::Refused { reason } => StoreError::Tampered {
            reason: reason.as_str(),
        },
        other => StoreError::Sealing {
            cause: other.into(),
        },
    }
}
