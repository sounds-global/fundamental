//! The port's contract, checked against an adapter.

use crate::{ByteRange, ByteStream, ObjectStore, StoreError};
use bytes::Bytes;
use futures::{StreamExt, stream};
use std::time::Duration;

/// The suite every [`ObjectStore`] adapter runs against itself; it asserts rather than returns errors.
pub struct Conformance;

impl Conformance {
    /// Checks `store` against the port's contract, under keys starting with `prefix` so runs
    /// against one bucket do not meet.
    ///
    /// # Panics
    /// When the store breaks the contract — that is the assertion.
    #[allow(clippy::missing_panics_doc, clippy::expect_used, clippy::panic)]
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::allow_attributes,
        clippy::allow_attributes_without_reason,
        reason = "readability baseline"
    )]
    pub async fn check<S: ObjectStore>(store: &S, prefix: &str) {
        assert!(!store.provider().as_str().is_empty());
        store
            .deletes_are_final()
            .await
            .expect("a conformance bucket keeps no old versions");

        let content: Vec<u8> = (0..300_000_u32).map(|i| (i % 251) as u8).collect();
        let key = format!("{prefix}/whole");
        let stored = store
            .put(&key, content.len() as u64, chunks(&content, 70_000))
            .await
            .expect("a put within its bound is kept");
        assert_eq!(
            stored,
            content.len() as u64,
            "a put answers the bytes stored"
        );
        assert_eq!(
            read(store, &key, None).await,
            content,
            "a get answers what was put"
        );
        assert_eq!(
            store.head(&key).await.expect("head"),
            Some(content.len() as u64)
        );

        let range = ByteRange::new(100_000, 100_009).expect("ordered");
        assert_eq!(
            read(store, &key, Some(range)).await,
            content[100_000..=100_009],
            "a ranged get answers exactly the range"
        );
        let body = store.get(&key, Some(range)).await.expect("ranged");
        assert_eq!(
            body.object_len,
            content.len() as u64,
            "a ranged get names the whole length"
        );

        let over = format!("{prefix}/over");
        let refused = store.put(&over, 10, chunks(&content[..11], 4)).await;
        assert!(
            matches!(refused, Err(StoreError::TooLarge { max: 10 })),
            "{refused:?}"
        );
        assert_eq!(
            store.head(&over).await.expect("head"),
            None,
            "an overrun keeps nothing"
        );

        let failed = format!("{prefix}/failed");
        let broken: ByteStream = Box::pin(stream::iter(vec![
            Ok(Bytes::from_static(b"partial")),
            Err(StoreError::BodyFailed {
                cause: StoreError::NotFound.into(),
            }),
        ]));
        let refused = store.put(&failed, 1_000, broken).await;
        assert!(
            matches!(refused, Err(StoreError::BodyFailed { .. })),
            "{refused:?}"
        );
        assert_eq!(
            store.head(&failed).await.expect("head"),
            None,
            "a failed body keeps nothing"
        );

        store.delete(&key).await.expect("delete");
        assert_eq!(store.head(&key).await.expect("head"), None);
        store
            .delete(&key)
            .await
            .expect("deleting an absent object succeeds");
        assert!(matches!(
            store.get(&key, None).await,
            Err(StoreError::NotFound)
        ));

        let url = store
            .presign_get(&format!("{prefix}/any"), Duration::from_secs(60))
            .expect("a store signs a read");
        assert!(!url.is_empty());
    }
}

fn chunks(content: &[u8], size: usize) -> ByteStream {
    let pieces: Vec<Result<Bytes, StoreError>> = content
        .chunks(size)
        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
        .collect();
    Box::pin(stream::iter(pieces))
}

#[allow(clippy::expect_used)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
async fn read<S: ObjectStore>(store: &S, key: &str, range: Option<ByteRange>) -> Vec<u8> {
    let mut body = store.get(key, range).await.expect("a get");
    let mut out = Vec::new();
    while let Some(chunk) = body.stream.next().await {
        out.extend_from_slice(&chunk.expect("a chunk"));
    }
    assert_eq!(
        out.len() as u64,
        body.len,
        "a body yields the length it names"
    );
    out
}
