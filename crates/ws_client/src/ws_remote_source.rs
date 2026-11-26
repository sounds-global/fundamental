//! [`WsRemoteSource`] — a [`RemoteSource`] backed by a WebSocket transport.

use fundamental_core::secrets::SecretString;
use fundamental_keys::{DeviceId, DeviceSigner};
use fundamental_sync::wire::{
    AuthAck, AuthRequest, SliceChange, SliceCursor, SlicePullRequest, SlicePullResponse,
    SliceSubscribeAck, SliceSubscribeRequest, SliceUpsertRequest, UpsertAck, WireRecord, action,
    canonical_bytes,
};
use fundamental_sync::{RemoteSource, SyncError, SyncRecord};
use fundamental_ws_wire::WsEnvelope;
use futures::Stream;
use tracing::field::Empty;
use tracing::{Span, info, instrument, warn};

use crate::TransportError;
use crate::connect_failure::ConnectFailure;
use crate::connection::Transport;
use crate::platform::PlatformTransport;
use crate::reconnect_hint::ReconnectHint;
use crate::unsigned::Unsigned;

#[cfg(feature = "vault-auth")]
use fundamental_keys::VaultOwnerKey;
#[cfg(feature = "vault-auth")]
use fundamental_sync::wire::{VaultAuthRequest, VaultChallengeRequest, VaultChallengeResponse};

/// A [`RemoteSource`] that reaches an authenticated server over one WebSocket.
///
/// `Sig` is the device signer: [`connect`](WsRemoteSource::connect) pushes unsigned,
/// [`connect_signed`](WsRemoteSource::connect_signed) signs every push.
pub struct WsRemoteSource<Sig = Unsigned> {
    transport: PlatformTransport,
    signing: Option<Signing<Sig>>,
}

/// A bound device signer and the id it signs as.
struct Signing<Sig> {
    device_id: DeviceId,
    signer: Sig,
}

impl<Sig> std::fmt::Debug for WsRemoteSource<Sig> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WsRemoteSource")
            .field("signed", &self.signing.is_some())
            .finish_non_exhaustive()
    }
}

impl WsRemoteSource<Unsigned> {
    /// Connects to the sync server at `url` (`ws://…` or `wss://…`), unsigned.
    ///
    /// # Errors
    /// Returns [`TransportError::Connect`] if the connection cannot be established.
    pub async fn connect(url: &str) -> Result<Self, TransportError> {
        Self::connect_hinted(url)
            .await
            .map_err(|failure| failure.error)
    }

    /// Connects, keeping a refused upgrade's retry hint beside the error.
    pub(crate) async fn connect_hinted(url: &str) -> Result<Self, ConnectFailure> {
        Ok(Self {
            transport: PlatformTransport::connect(url).await?,
            signing: None,
        })
    }

    pub(crate) fn set_request_timeout(&mut self, timeout: std::time::Duration) {
        self.transport.set_request_timeout(timeout);
    }

    /// Connects to `url` on the reconnect schedule, honouring `Retry-After`, until
    /// `max_attempts` consecutive failures. Native only.
    ///
    /// # Errors
    /// Returns the last [`TransportError::Connect`] once the retries are exhausted.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn connect_with_retry(url: &str, max_attempts: u32) -> Result<Self, TransportError> {
        Ok(Self {
            transport: PlatformTransport::connect_with_retry(url, max_attempts).await?,
            signing: None,
        })
    }
}

impl<Sig: DeviceSigner> WsRemoteSource<Sig> {
    /// Connects to `url` and signs every push with `signer` as `device_id`.
    ///
    /// # Errors
    /// Returns [`TransportError::Connect`] if the connection cannot be established.
    pub async fn connect_signed(
        url: &str,
        device_id: DeviceId,
        signer: Sig,
    ) -> Result<Self, TransportError> {
        Ok(Self {
            transport: PlatformTransport::connect(url)
                .await
                .map_err(|failure| failure.error)?,
            signing: Some(Signing { device_id, signer }),
        })
    }

    /// Authenticates the connection by presenting `token` as the first frame.
    ///
    /// Call it once, before syncing.
    ///
    /// # Errors
    /// Returns [`TransportError::Protocol`] if the server rejects the token, or a
    /// transport error if the connection fails.
    #[instrument(skip(self, token), err(Debug))]
    pub async fn authenticate(&self, token: &SecretString) -> Result<(), TransportError> {
        let request_id = self.transport.next_request_id();
        let request = AuthRequest::new(request_id, token.expose());
        let data =
            serde_json::to_value(&request).map_err(|e| TransportError::Codec(e.to_string()))?;
        let value = self
            .transport
            .request(request_id, WsEnvelope::new(action::AUTH, data))
            .await?;
        let ack: AuthAck = serde_json::from_value(value)
            .map_err(|e| TransportError::Codec(format!("decoding auth ack: {e}")))?;
        if !ack.ok {
            return Err(TransportError::Protocol(
                "authentication rejected".to_string(),
            ));
        }
        info!("connection authenticated");
        Ok(())
    }

    /// Signs `(slice, record)` if a device key is attached; empty strings when unsigned.
    async fn sign(&self, slice: &str, record: &SyncRecord) -> Result<(String, String), SyncError> {
        match &self.signing {
            None => Ok((String::new(), String::new())),
            Some(signing) => {
                let canonical = canonical_bytes(slice, record);
                let signature = signing
                    .signer
                    .sign(&canonical)
                    .await
                    .map_err(|e| SyncError::Push(format!("signing push: {e}")))?;
                Ok((
                    signing.device_id.as_str().to_string(),
                    signature.to_string(),
                ))
            }
        }
    }

    /// Pulls a named slice's changed rows since `since`, across all pages.
    /// # Errors
    /// Returns [`SyncError::Pull`] on a transport or decode failure, or
    /// [`SyncError::Protocol`] if a returned row fails validation.
    #[instrument(skip(self, params), fields(slice = %slice, since), err(Debug))]
    pub async fn pull_slice(
        &self,
        slice: &str,
        params: serde_json::Value,
        since: i64,
    ) -> Result<Vec<SyncRecord>, SyncError> {
        pull_slice_all(&self.transport, slice, params, since).await
    }

    /// Subscribes to live changes for `slice`: from now on the server pushes a
    /// change for every in-scope update, drained through [`live_changes`](Self::live_changes).
    ///
    ///
    /// Subscribing does not replay history; backfill with [`pull_slice`](Self::pull_slice).
    ///
    /// # Errors
    /// Returns [`SyncError::Pull`] on a transport or decode failure, or if the
    /// server rejects the subscription.
    #[instrument(skip(self, params), fields(slice = %slice), err(Debug))]
    pub async fn subscribe_slice(
        &self,
        slice: &str,
        params: serde_json::Value,
    ) -> Result<(), SyncError> {
        let request_id = self.transport.next_request_id();
        let request = SliceSubscribeRequest::new(request_id, slice, params);
        let data = serde_json::to_value(&request)
            .map_err(|e| SyncError::Pull(format!("encoding subscribe: {e}")))?;
        let value = self
            .transport
            .request(request_id, WsEnvelope::new(action::SLICE_SUBSCRIBE, data))
            .await
            .map_err(|e| SyncError::Pull(e.to_string()))?;
        let ack: SliceSubscribeAck = serde_json::from_value(value)
            .map_err(|e| SyncError::Pull(format!("decoding subscribe ack: {e}")))?;
        if !ack.subscribed {
            return Err(SyncError::Pull("subscription rejected".to_string()));
        }
        info!(slice, "subscribed to live changes");
        Ok(())
    }

    /// Returns the stream of [`SliceChange`]s pushed for subscribed slices; `None` once taken.
    pub fn live_changes(&self) -> Option<impl Stream<Item = SliceChange>> {
        self.transport.take_live_changes()
    }

    /// The retry hint the server's close frame carried (1001 or 1013).
    pub(crate) fn close_hint(&self) -> Option<ReconnectHint> {
        self.transport.close_hint()
    }

    /// Whether the connection is gone: closed, or a request went unanswered past its deadline.
    pub(crate) fn is_lost(&self) -> bool {
        self.transport.is_lost()
    }

    /// Pushes a locally-changed `record` back through a named slice.
    ///
    /// The server refuses it unless the slice is bidirectional and the record in scope.
    ///
    /// # Errors
    /// Returns [`SyncError::Push`] on a transport failure or if the server rejects
    /// the write (pull-only slice, out-of-scope record, …).
    #[instrument(skip(self, params, record), fields(slice = %slice, id = %record.id), err(Debug))]
    pub async fn push_slice(
        &self,
        slice: &str,
        params: serde_json::Value,
        record: &SyncRecord,
    ) -> Result<(), SyncError> {
        let (device_id, signature) = self.sign(slice, record).await?;
        push_slice_one(&self.transport, slice, params, record, device_id, signature).await
    }
}

/// The `vault-auth` challenge/response; it does not use the device signer.
#[cfg(feature = "vault-auth")]
impl<Sig> WsRemoteSource<Sig> {
    /// Authenticates the connection as the vault `owner` by signing a single-use nonce;
    /// call it once, before syncing.
    ///
    /// # Errors
    /// [`TransportError::Protocol`] if the server rejects the proof (bad key/signature,
    /// or an expired/replayed nonce), [`TransportError::Codec`] on a malformed frame, or
    /// a transport error if the connection fails.
    #[instrument(skip(self, owner), err(Debug))]
    pub async fn authenticate_vault(&self, owner: &VaultOwnerKey) -> Result<(), TransportError> {
        authenticate_vault_over(&self.transport, owner).await
    }
}

/// Runs the vault challenge/response over any [`Transport`].
///
/// # Errors
/// As [`WsRemoteSource::authenticate_vault`].
#[cfg(feature = "vault-auth")]
#[instrument(skip(transport, owner), err(Debug))]
pub(crate) async fn authenticate_vault_over<T: Transport>(
    transport: &T,
    owner: &VaultOwnerKey,
) -> Result<(), TransportError> {
    let challenge_id = transport.next_request_id();
    let challenge_req = VaultChallengeRequest::new(challenge_id);
    let data =
        serde_json::to_value(&challenge_req).map_err(|e| TransportError::Codec(e.to_string()))?;
    let value = transport
        .request(challenge_id, WsEnvelope::new(action::VAULT_CHALLENGE, data))
        .await?;
    let challenge: VaultChallengeResponse = serde_json::from_value(value)
        .map_err(|e| TransportError::Codec(format!("decoding vault challenge: {e}")))?;

    let signature = owner.sign_challenge(&challenge.nonce);
    let auth_id = transport.next_request_id();
    let auth_req = VaultAuthRequest::new(
        auth_id,
        owner.public_key().to_string(),
        challenge.nonce,
        signature.to_string(),
    );
    let data = serde_json::to_value(&auth_req).map_err(|e| TransportError::Codec(e.to_string()))?;
    let value = transport
        .request(auth_id, WsEnvelope::new(action::VAULT_AUTH, data))
        .await?;
    let ack: AuthAck = serde_json::from_value(value)
        .map_err(|e| TransportError::Codec(format!("decoding vault auth ack: {e}")))?;
    if !ack.ok {
        return Err(TransportError::Protocol(
            "vault authentication rejected".to_string(),
        ));
    }
    info!("vault connection authenticated");
    Ok(())
}

/// Pushes one row back through a slice and waits for its acknowledgement.
///
/// `device_id` and `signature` are empty for an unsigned client.
///
/// # Errors
/// Returns [`SyncError::Push`] on a transport failure or if the server rejects the
/// write.
#[instrument(skip(transport, params, record, signature), fields(slice = %slice, id = %record.id, request_id = Empty), err(Debug))]
#[expect(clippy::too_many_arguments, reason = "readability baseline")]
pub(crate) async fn push_slice_one<T: Transport>(
    transport: &T,
    slice: &str,
    params: serde_json::Value,
    record: &SyncRecord,
    device_id: String,
    signature: String,
) -> Result<(), SyncError> {
    let request_id = transport.next_request_id();
    Span::current().record("request_id", request_id);
    let request = SliceUpsertRequest::new(request_id, slice, params, WireRecord::from(record))
        .signed(device_id, signature);
    let data = serde_json::to_value(&request).map_err(|e| SyncError::Push(e.to_string()))?;
    let value = transport
        .request(request_id, WsEnvelope::new(action::SLICE_UPSERT, data))
        .await
        .map_err(|e| SyncError::Push(e.to_string()))?;
    let ack: UpsertAck = serde_json::from_value(value)
        .map_err(|e| SyncError::Push(format!("decoding slice upsert ack: {e}")))?;
    if !ack.ok {
        return Err(SyncError::Push(format!(
            "server rejected slice upsert of `{}`",
            record.id
        )));
    }
    Ok(())
}

impl<Sig: DeviceSigner> RemoteSource for WsRemoteSource<Sig> {
    async fn changed_since(
        &self,
        slice: &str,
        params: &serde_json::Value,
        since: i64,
    ) -> Result<Vec<SyncRecord>, SyncError> {
        self.pull_slice(slice, params.clone(), since).await
    }

    async fn upsert(
        &self,
        slice: &str,
        params: &serde_json::Value,
        record: &SyncRecord,
    ) -> Result<(), SyncError> {
        self.push_slice(slice, params.clone(), record).await
    }
}

/// Pulls every row of a slice changed since `since`, across all pages.
///
/// # Errors
/// Returns [`SyncError::Pull`] on a transport or decode failure, or
/// [`SyncError::Protocol`] if a returned row fails validation.
#[instrument(skip(transport, params), fields(slice = %slice, since, pages = Empty), err(Debug))]
pub(crate) async fn pull_slice_all<T: Transport>(
    transport: &T,
    slice: &str,
    params: serde_json::Value,
    since: i64,
) -> Result<Vec<SyncRecord>, SyncError> {
    let mut records = Vec::new();
    let mut cursor: Option<SliceCursor> = None;
    let mut pages = 0u32;
    loop {
        let request_id = transport.next_request_id();
        let mut request = SlicePullRequest::new(request_id, slice, params.clone(), since);
        if let Some(cursor) = cursor.take() {
            request = request.with_cursor(cursor);
        }
        let data = serde_json::to_value(&request).map_err(|e| SyncError::Pull(e.to_string()))?;
        let value = transport
            .request(request_id, WsEnvelope::new(action::SLICE_PULL, data))
            .await
            .map_err(|e| SyncError::Pull(e.to_string()))?;
        let response: SlicePullResponse = serde_json::from_value(value)
            .map_err(|e| SyncError::Pull(format!("decoding slice pull response: {e}")))?;
        pages += 1;
        for wire in response.records {
            records.push(SyncRecord::try_from(wire)?);
        }
        if !response.has_more {
            break;
        }
        match response.next_cursor {
            Some(next) => cursor = Some(next),
            None => {
                warn!(
                    request_id,
                    "server set has_more without a next_cursor; stopping"
                );
                break;
            }
        }
    }
    Span::current().record("pages", pages);
    Ok(records)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU64, Ordering};

    use fundamental_core::Error as _;
    use fundamental_sync::SyncMeta;
    use serde_json::Value;

    use super::*;

    /// A scripted [`Transport`] that records each request it is sent.
    #[derive(Default)]
    struct FakeTransport {
        next_id: AtomicU64,
        replies: RefCell<VecDeque<Result<Value, TransportError>>>,
        sent: RefCell<Vec<WsEnvelope>>,
    }

    impl FakeTransport {
        fn with_replies(replies: Vec<Result<Value, TransportError>>) -> Self {
            Self {
                next_id: AtomicU64::new(1),
                replies: RefCell::new(replies.into()),
                sent: RefCell::new(Vec::new()),
            }
        }
    }

    impl Transport for FakeTransport {
        fn next_request_id(&self) -> u64 {
            self.next_id.fetch_add(1, Ordering::Relaxed)
        }

        async fn request(
            &self,
            _request_id: u64,
            envelope: WsEnvelope,
        ) -> Result<Value, TransportError> {
            self.sent.borrow_mut().push(envelope);
            self.replies
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Err(TransportError::Closed))
        }

        fn take_live_changes(&self) -> Option<crate::connection::LiveReceiver> {
            None
        }

        fn close_hint(&self) -> Option<ReconnectHint> {
            None
        }

        fn set_request_timeout(&mut self, _timeout: std::time::Duration) {}

        fn is_lost(&self) -> bool {
            false
        }
    }

    fn wire_rows(ids: &[&str]) -> Vec<WireRecord> {
        ids.iter()
            .enumerate()
            .map(|(i, id)| {
                WireRecord::from(&SyncRecord::new(
                    (*id).to_string(),
                    SyncMeta::new(i as i64 + 1, 1),
                    Default::default(),
                ))
            })
            .collect()
    }

    fn json(value: impl serde::Serialize) -> Value {
        serde_json::to_value(value).expect("serialize")
    }

    #[tokio::test]
    async fn pull_aggregates_a_single_page() {
        let transport = FakeTransport::with_replies(vec![Ok(json(SlicePullResponse::last(
            1,
            wire_rows(&["a", "b"]),
        )))]);
        let records = pull_slice_all(&transport, "mine", Value::Null, 0)
            .await
            .expect("pull");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, "a");
        assert_eq!(transport.sent.borrow().len(), 1);
    }

    #[tokio::test]
    async fn pull_follows_the_cursor_across_pages_in_order() {
        let transport = FakeTransport::with_replies(vec![
            Ok(json(SlicePullResponse::page(
                1,
                wire_rows(&["a", "b"]),
                SliceCursor::new(50, "b"),
            ))),
            Ok(json(SlicePullResponse::last(2, wire_rows(&["c"])))),
        ]);
        let records = pull_slice_all(&transport, "mine", Value::Null, 0)
            .await
            .expect("pull");
        assert_eq!(
            records.iter().map(|r| r.id.to_string()).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
        let sent = transport.sent.borrow();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1].data["cursor"]["updated_at"], 50);
    }

    #[tokio::test]
    async fn pull_stops_when_has_more_lacks_a_cursor() {
        // has_more without next_cursor must not loop.
        let mut page = SlicePullResponse::last(1, wire_rows(&["a"]));
        page.has_more = true;
        let transport = FakeTransport::with_replies(vec![Ok(json(page))]);
        let records = pull_slice_all(&transport, "mine", Value::Null, 0)
            .await
            .expect("pull");
        assert_eq!(records.len(), 1);
        assert_eq!(transport.sent.borrow().len(), 1);
    }

    #[tokio::test]
    async fn pull_maps_a_transport_error_to_sync_pull() {
        let transport =
            FakeTransport::with_replies(vec![Err(TransportError::Protocol("boom".to_string()))]);
        let err = pull_slice_all(&transport, "mine", Value::Null, 0)
            .await
            .expect_err("must fail");
        assert!(matches!(err, SyncError::Pull(_)));
        assert_eq!(err.error_code(), "SYNC_002");
    }

    #[tokio::test]
    async fn pull_maps_a_closed_connection_to_sync_pull() {
        let transport = FakeTransport::with_replies(vec![Err(TransportError::Closed)]);
        let err = pull_slice_all(&transport, "mine", Value::Null, 0)
            .await
            .expect_err("must fail");
        assert!(matches!(err, SyncError::Pull(_)));
    }

    #[tokio::test]
    async fn pull_rejects_an_invalid_row_with_protocol() {
        let bad = json(serde_json::json!({
            "request_id": 1,
            "records": [{ "id": "x", "meta": { "updated_at": -1, "version": 0, "tombstone": false } }],
            "has_more": false
        }));
        let transport = FakeTransport::with_replies(vec![Ok(bad)]);
        let err = pull_slice_all(&transport, "mine", Value::Null, 0)
            .await
            .expect_err("must fail");
        assert!(matches!(err, SyncError::Protocol(_)));
        assert_eq!(err.error_code(), "SYNC_005");
    }

    #[tokio::test]
    async fn push_sends_an_upsert_and_accepts_the_ack() {
        let transport = FakeTransport::with_replies(vec![Ok(json(UpsertAck::applied(1)))]);
        let record = SyncRecord::new("row-1", SyncMeta::new(5, 1), Default::default());
        push_slice_one(
            &transport,
            "mine",
            Value::Null,
            &record,
            String::new(),
            String::new(),
        )
        .await
        .expect("push");
        let sent = transport.sent.borrow();
        assert_eq!(sent[0].action, action::SLICE_UPSERT);
        assert_eq!(sent[0].data["record"]["id"], "row-1");
    }

    #[cfg(feature = "vault-auth")]
    #[tokio::test]
    async fn authenticate_vault_signs_the_nonce_and_binds_on_ack() {
        use fundamental_core::secrets::Secret;
        use fundamental_keys::{VaultOwnerKey, verify_owner_challenge};

        let nonce = vec![0x11u8; 32];
        let transport = FakeTransport::with_replies(vec![
            Ok(json(VaultChallengeResponse::new(1, nonce.clone(), 9_999))),
            Ok(json(AuthAck::authenticated(2))),
        ]);
        let owner = VaultOwnerKey::from_child_seed(&Secret::new([0x42u8; 32]));
        super::authenticate_vault_over(&transport, &owner)
            .await
            .expect("vault auth");

        let sent = transport.sent.borrow();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].action, action::VAULT_CHALLENGE);
        assert_eq!(sent[1].action, action::VAULT_AUTH);
        let auth: VaultAuthRequest = serde_json::from_value(sent[1].data.clone()).expect("auth");
        assert_eq!(auth.nonce, nonce);
        let key = auth.owner_public_key.parse().expect("key");
        let sig = auth.signature.parse().expect("sig");
        verify_owner_challenge(&key, &auth.nonce, &sig).expect("signature verifies");
    }

    #[cfg(feature = "vault-auth")]
    #[tokio::test]
    async fn authenticate_vault_maps_a_rejected_proof_to_protocol() {
        use fundamental_core::secrets::Secret;
        use fundamental_keys::VaultOwnerKey;

        let transport = FakeTransport::with_replies(vec![
            Ok(json(VaultChallengeResponse::new(
                1,
                vec![0x22u8; 32],
                9_999,
            ))),
            Ok(json(AuthAck {
                request_id: 2,
                ok: false,
            })),
        ]);
        let owner = VaultOwnerKey::from_child_seed(&Secret::new([0x43u8; 32]));
        let err = super::authenticate_vault_over(&transport, &owner)
            .await
            .expect_err("must reject");
        assert!(matches!(err, TransportError::Protocol(_)));
    }

    #[tokio::test]
    async fn push_maps_a_rejected_ack_to_sync_push() {
        let transport = FakeTransport::with_replies(vec![Ok(json(UpsertAck {
            request_id: 1,
            ok: false,
        }))]);
        let record = SyncRecord::new("row-1", SyncMeta::new(5, 1), Default::default());
        let err = push_slice_one(
            &transport,
            "mine",
            Value::Null,
            &record,
            String::new(),
            String::new(),
        )
        .await
        .expect_err("must fail");
        assert!(matches!(err, SyncError::Push(_)));
        assert!(err.to_string().contains("row-1"));
    }
}
