//! A private bucket holds only ciphertext, and gives content back only when it opens intact.

use bytes::Bytes;
use fundamental_encryption::{ContentKey, StreamLayout};
use fundamental_storage::{
    Bucket, ByteRange, ByteStream, MemoryObjectStore, ObjectKey, Private, Public, PublicArtefact,
    StoreError,
};
use futures::{StreamExt, stream};

const SEGMENT: usize = StreamLayout::SEGMENT_LEN;

fn content(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 249) as u8).collect()
}

fn chunked(bytes: &[u8], size: usize) -> ByteStream {
    let pieces: Vec<Result<Bytes, StoreError>> = bytes
        .chunks(size.max(1))
        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
        .collect();
    Box::pin(stream::iter(pieces))
}

async fn collect(mut stream: ByteStream) -> Result<Vec<u8>, StoreError> {
    let mut out = Vec::new();
    while let Some(chunk) = stream.next().await {
        out.extend_from_slice(&chunk?);
    }
    Ok(out)
}

fn bucket(store: &MemoryObjectStore) -> Bucket<Private, MemoryObjectStore> {
    Bucket::new(vec![store.clone()]).expect("one store")
}

#[tokio::test]
async fn the_provider_holds_only_ciphertext() {
    let store = MemoryObjectStore::new();
    let bucket = bucket(&store);
    let key = ObjectKey::<Private>::random().expect("entropy");
    let content_key = ContentKey::generate().expect("entropy");
    let marker = b"a sentence a person wrote in their file".repeat(5_000);
    let stored = bucket
        .put_sealed(
            &key,
            &content_key,
            &[b"upload-1"],
            1 << 20,
            chunked(&marker, 7_000),
        )
        .await
        .expect("stored");
    assert_eq!(stored.content_len, marker.len() as u64);

    let raw = store.raw(key.as_str()).expect("an object");
    assert_eq!(
        Some(raw.len() as u64),
        StreamLayout::ciphertext_len(marker.len() as u64)
    );
    assert!(
        !raw.windows(24)
            .any(|window| marker.windows(24).take(64).any(|m| m == window)),
        "the stored bytes contain the content"
    );

    let body = bucket
        .open(
            stored.shard,
            &key,
            &content_key,
            &[b"upload-1"],
            stored.content_len,
            None,
        )
        .await
        .expect("opens");
    assert_eq!(collect(body.stream).await.expect("intact"), marker);
}

#[tokio::test]
async fn content_round_trips_at_every_boundary() {
    let store = MemoryObjectStore::new();
    let bucket = bucket(&store);
    for len in [0, 1, SEGMENT - 1, SEGMENT, SEGMENT + 1, 5 * SEGMENT + 17] {
        let key = ObjectKey::<Private>::random().expect("entropy");
        let content_key = ContentKey::generate().expect("entropy");
        let bytes = content(len);
        let stored = bucket
            .put_sealed(&key, &content_key, &[], 1 << 24, chunked(&bytes, 50_000))
            .await
            .expect("stored");
        let body = bucket
            .open(
                stored.shard,
                &key,
                &content_key,
                &[],
                stored.content_len,
                None,
            )
            .await
            .expect("opens");
        assert_eq!(body.len, len as u64);
        assert_eq!(collect(body.stream).await.expect("intact"), bytes, "{len}");
    }
}

#[tokio::test]
async fn a_range_is_decrypted_and_trimmed() {
    let store = MemoryObjectStore::new();
    let bucket = bucket(&store);
    let key = ObjectKey::<Private>::random().expect("entropy");
    let content_key = ContentKey::generate().expect("entropy");
    let bytes = content(4 * SEGMENT + 300);
    let stored = bucket
        .put_sealed(&key, &content_key, &[b"x"], 1 << 24, chunked(&bytes, 9_999))
        .await
        .expect("stored");
    for (first, last) in [
        (0, 0),
        (5, SEGMENT as u64 - 1),
        (SEGMENT as u64 - 3, SEGMENT as u64 + 3),
        (2 * SEGMENT as u64 + 7, 4 * SEGMENT as u64 + 299),
        (4 * SEGMENT as u64 + 299, 9 * SEGMENT as u64),
    ] {
        let range = ByteRange::new(first, last).expect("ordered");
        let body = bucket
            .open(
                stored.shard,
                &key,
                &content_key,
                &[b"x"],
                stored.content_len,
                Some(range),
            )
            .await
            .expect("opens");
        let clamped = range.within(bytes.len() as u64).expect("satisfiable");
        assert_eq!(body.len, clamped.len());
        let expected = &bytes[clamped.first() as usize..=clamped.last() as usize];
        assert_eq!(
            collect(body.stream).await.expect("intact"),
            expected,
            "{first}..={last}"
        );
    }
    let past = ByteRange::new(bytes.len() as u64, bytes.len() as u64 + 5).expect("ordered");
    assert!(matches!(
        bucket
            .open(
                stored.shard,
                &key,
                &content_key,
                &[b"x"],
                stored.content_len,
                Some(past)
            )
            .await,
        Err(StoreError::InvalidSettings { .. })
    ));
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn a_swapped_truncated_or_extended_object_is_refused() {
    let store = MemoryObjectStore::new();
    let bucket = bucket(&store);
    let content_key = ContentKey::generate().expect("entropy");
    let bytes = content(2 * SEGMENT + 10);
    let put = |key: ObjectKey<Private>| {
        let bucket = &bucket;
        let content_key = &content_key;
        let bytes = &bytes;
        async move {
            bucket
                .put_sealed(
                    &key,
                    content_key,
                    &[b"same"],
                    1 << 24,
                    chunked(bytes, 8_192),
                )
                .await
                .expect("stored")
        }
    };
    let first = ObjectKey::<Private>::random().expect("entropy");
    let second = ObjectKey::<Private>::random().expect("entropy");
    let stored = put(first.clone()).await;
    put(second.clone()).await;
    let original = store.raw(first.as_str()).expect("held");

    let open = || {
        bucket.open(
            stored.shard,
            &first,
            &content_key,
            &[b"same"],
            stored.content_len,
            None,
        )
    };
    let refused = |result: Result<Vec<u8>, StoreError>| {
        assert!(
            matches!(result, Err(StoreError::Tampered { .. })),
            "{result:?}"
        );
    };

    // Another object sealed under the same key and binding.
    store.tamper(first.as_str(), store.raw(second.as_str()).expect("held"));
    refused(match open().await {
        Ok(body) => collect(body.stream).await,
        Err(error) => Err(error),
    });

    // Cut short, and extended.
    for altered in [original.slice(..original.len() - 1), {
        let mut longer = original.to_vec();
        longer.extend_from_slice(&[0; 32]);
        Bytes::from(longer)
    }] {
        store.tamper(first.as_str(), altered);
        refused(match open().await {
            Ok(body) => collect(body.stream).await,
            Err(error) => Err(error),
        });
    }

    // One flipped bit in the middle segment.
    let mut flipped = original.to_vec();
    flipped[StreamLayout::HEADER_LEN + SEGMENT + 40] ^= 0x10;
    store.tamper(first.as_str(), Bytes::from(flipped));
    refused(match open().await {
        Ok(body) => collect(body.stream).await,
        Err(error) => Err(error),
    });

    // Intact, but read under another binding.
    store.tamper(first.as_str(), original);
    let other = bucket
        .open(
            stored.shard,
            &first,
            &content_key,
            &[b"other"],
            stored.content_len,
            None,
        )
        .await;
    refused(match other {
        Ok(body) => collect(body.stream).await,
        Err(error) => Err(error),
    });
}

#[tokio::test]
async fn content_past_its_bound_leaves_no_object() {
    let store = MemoryObjectStore::new();
    let bucket = bucket(&store);
    let key = ObjectKey::<Private>::random().expect("entropy");
    let content_key = ContentKey::generate().expect("entropy");
    let result = bucket
        .put_sealed(
            &key,
            &content_key,
            &[],
            1_000,
            chunked(&content(1_001), 100),
        )
        .await;
    assert!(
        matches!(result, Err(StoreError::TooLarge { max: 1_000 })),
        "{result:?}"
    );
    assert!(store.keys().is_empty());
}

#[tokio::test]
async fn an_unreachable_store_is_retryable() {
    use fundamental_core::Error as _;
    let store = MemoryObjectStore::new();
    store.set_unreachable(true);
    let bucket = bucket(&store);
    let key = ObjectKey::<Private>::random().expect("entropy");
    let content_key = ContentKey::generate().expect("entropy");
    let error = bucket
        .put_sealed(&key, &content_key, &[], 10, chunked(b"x", 1))
        .await
        .expect_err("unreachable");
    assert_eq!(error.error_code(), "BLOB_002");
    assert!(error.is_retryable());
}

#[tokio::test]
async fn a_versioned_or_locked_bucket_refuses_to_boot() {
    let store = MemoryObjectStore::new();
    store.set_versioned(true);
    assert!(matches!(
        bucket(&store).verify().await,
        Err(StoreError::InvalidSettings { .. })
    ));
    store.set_versioned(false);
    bucket(&store).verify().await.expect("final deletes");
}

#[tokio::test]
async fn a_public_artefact_is_stored_as_it_is() {
    let store = MemoryObjectStore::new();
    let bucket: Bucket<Public, _> = Bucket::new(vec![store.clone()]).expect("one store");
    let key = ObjectKey::<Public>::parse("app/1.0.0/latest.json").expect("a public key");
    let manifest = b"{\"version\":\"1.0.0\"}".to_vec();
    let stored = bucket
        .put(&key, PublicArtefact::new(1_024, chunked(&manifest, 5)))
        .await
        .expect("stored");
    assert_eq!(store.raw(key.as_str()).as_deref(), Some(&manifest[..]));
    let url = bucket
        .presign_get(stored.shard, &key, std::time::Duration::from_secs(60))
        .expect("signed");
    assert!(url.contains("app/1.0.0/latest.json"));
}

#[tokio::test]
async fn a_bucket_spreads_keys_and_keeps_each_on_its_shard() {
    let stores: Vec<MemoryObjectStore> = (0..4).map(|_| MemoryObjectStore::new()).collect();
    let bucket: Bucket<Private, _> = Bucket::new(stores.clone()).expect("four stores");
    let content_key = ContentKey::generate().expect("entropy");
    let mut used = std::collections::BTreeSet::new();
    for _ in 0..64 {
        let key = ObjectKey::<Private>::random().expect("entropy");
        let stored = bucket
            .put_sealed(&key, &content_key, &[], 10, chunked(b"x", 1))
            .await
            .expect("stored");
        used.insert(stored.shard.get());
        assert!(
            stores[usize::from(stored.shard.get())]
                .raw(key.as_str())
                .is_some()
        );
    }
    assert_eq!(used.len(), 4, "sixty-four keys reach every store");
    assert!(Bucket::<Private, MemoryObjectStore>::new(Vec::new()).is_err());
}

#[derive(Clone)]
struct MiscountingStore(MemoryObjectStore);

impl fundamental_storage::ObjectStore for MiscountingStore {
    fn provider(&self) -> fundamental_core::ProviderName {
        self.0.provider()
    }

    async fn put(&self, key: &str, max_len: u64, body: ByteStream) -> Result<u64, StoreError> {
        Ok(self.0.put(key, max_len, body).await? + 1)
    }

    async fn get(
        &self,
        key: &str,
        range: Option<ByteRange>,
    ) -> Result<fundamental_storage::ObjectBody, StoreError> {
        self.0.get(key, range).await
    }

    async fn head(&self, key: &str) -> Result<Option<u64>, StoreError> {
        self.0.head(key).await
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.0.delete(key).await
    }

    fn presign_get(&self, key: &str, ttl: std::time::Duration) -> Result<String, StoreError> {
        self.0.presign_get(key, ttl)
    }

    async fn deletes_are_final(&self) -> Result<(), StoreError> {
        self.0.deletes_are_final().await
    }
}

#[tokio::test]
async fn a_store_answering_another_length_is_a_bad_reply() {
    let store = MiscountingStore(MemoryObjectStore::new());
    let bucket: Bucket<Private, _> = Bucket::new(vec![store]).expect("one store");
    let key = ObjectKey::<Private>::random().expect("entropy");
    let Err(refused) = bucket
        .put_sealed(
            &key,
            &ContentKey::generate().expect("entropy"),
            &[b"x"],
            1 << 20,
            chunked(&content(1000), 100),
        )
        .await
    else {
        panic!("a length other than the sealed one is refused");
    };
    assert!(
        matches!(refused, StoreError::BadReply { .. }),
        "{refused:?}"
    );
    assert_eq!(fundamental_core::Error::error_code(&refused), "BLOB_005");
}

#[tokio::test]
async fn a_range_past_the_object_is_refused_by_the_provider() {
    use fundamental_storage::ObjectStore as _;

    let store = MemoryObjectStore::new();
    store
        .put("public/a", 10, chunked(b"0123456789", 4))
        .await
        .expect("stored");
    let Err(refused) = store
        .get("public/a", Some(ByteRange::new(20, 29).expect("ordered")))
        .await
    else {
        panic!("a range wholly past the object");
    };
    assert!(
        matches!(refused, StoreError::Refused { status: 416, .. }),
        "{refused:?}"
    );
    assert_eq!(fundamental_core::Error::error_code(&refused), "BLOB_004");
}
