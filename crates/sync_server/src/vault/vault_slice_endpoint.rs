use std::sync::Arc;

use fundamental_auth::{AuthError, AuthenticatedPrincipal, UserId};
use fundamental_core::Error as _;
use fundamental_sync::wire::{
    AuthAck, SlicePullRequest, SlicePullResponse, SliceUpsertRequest, UpsertAck, VaultAuthRequest,
    VaultChallengeRequest, VaultChallengeResponse, WireRecord, action,
};
use fundamental_sync::{SliceWriteStore, SyncRecord, VaultError};
use fundamental_ws::{Route, WsContext, WsError, WsResult, WsRouter};
use tracing::{debug, error, info};

use crate::RateLimiter;
use crate::SliceServerSource;
use crate::signed_endpoint::now_ms;
use crate::sync_endpoint::{check_rate, reply_error, versioned_route};
use crate::vault::{VaultChallengeGauge, VaultHandshake, VaultPrincipal, authenticate_vault};

/// The page size used when a vault `slice.pull` omits `limit`.
const DEFAULT_PAGE_LIMIT: u32 = 500;
/// The largest page a vault `slice.pull` returns.
const MAX_PAGE_LIMIT: u32 = 1000;
/// How long an issued challenge nonce stays valid.
const CHALLENGE_TTL_MS: i64 = 60_000;
/// The length of a vault challenge nonce.
const NONCE_LEN: usize = 32;

/// Builds the E2EE-vault slice sync router.
///
/// The connection authenticates with `vault.challenge` then `vault.auth`; `max_payload_bytes`
/// bounds one sealed row's payload, and `gauge` counts pending challenges.
pub fn vault_slice_sync_router<S>(
    gauge: Arc<VaultChallengeGauge>,
    source: Arc<SliceServerSource<S>>,
    limiter: Arc<RateLimiter>,
    max_payload_bytes: usize,
) -> WsRouter
where
    S: SliceWriteStore + Send + Sync + 'static,
{
    WsRouter::new()
        .route(vault_challenge_route(gauge))
        .route(vault_auth_route())
        .route(vault_pull_route(source.clone(), limiter.clone()))
        .route(vault_upsert_route(source, limiter, max_payload_bytes))
}

fn vault_challenge_route(gauge: Arc<VaultChallengeGauge>) -> Route {
    versioned_route(
        action::VAULT_CHALLENGE,
        move |ctx: WsContext, req: VaultChallengeRequest| {
            let gauge = gauge.clone();
            async move { handle_vault_challenge(&ctx, req, gauge).await }
        },
    )
}

async fn handle_vault_challenge(
    ctx: &WsContext,
    req: VaultChallengeRequest,
    gauge: Arc<VaultChallengeGauge>,
) -> WsResult<()> {
    let handshake = connection_handshake(ctx, gauge)?;
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce).map_err(|error| {
        error!(%error, "vault challenge: the OS CSPRNG failed");
        WsError::internal("vault challenge entropy unavailable")
    })?;
    match handshake.issue(nonce, now_ms(), CHALLENGE_TTL_MS) {
        Ok(challenge) => {
            debug!("vault challenge issued");
            ctx.send(VaultChallengeResponse::new(
                req.request_id,
                challenge.nonce().to_vec(),
                challenge.expires_at_ms(),
            ))
            .await
        }
        Err(error) => reply_error(ctx, req.request_id, &error).await,
    }
}

/// The connection's vault handshake, placed in its state slot by its first challenge.
///
/// Dispatch is sequential per connection, so a failed set means another router holds the slot.
fn connection_handshake(
    ctx: &WsContext,
    gauge: Arc<VaultChallengeGauge>,
) -> WsResult<&VaultHandshake> {
    if ctx.state::<VaultHandshake>().is_none() {
        ctx.set_state(VaultHandshake::new(gauge))?;
    }
    ctx.state::<VaultHandshake>()
        .ok_or_else(|| WsError::internal("connection state is not a vault handshake"))
}

fn vault_auth_route() -> Route {
    versioned_route(
        action::VAULT_AUTH,
        move |ctx: WsContext, req: VaultAuthRequest| async move { handle_vault_auth(&ctx, req).await },
    )
}

#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn handle_vault_auth(ctx: &WsContext, req: VaultAuthRequest) -> WsResult<()> {
    let request_id = req.request_id;
    let Some(handshake) = ctx.state::<VaultHandshake>() else {
        debug!("vault answer refused: the connection asked for no challenge");
        return reply_error(ctx, request_id, &VaultError::OwnerProofFailed).await;
    };
    match authenticate_vault(handshake, &req, now_ms()) {
        // Proven, not admitted: a line here would be one per minted key.
        Ok(_) => {
            debug!("vault owner proven");
            ctx.send(AuthAck::authenticated(request_id)).await
        }
        Err(error) => reply_error(ctx, request_id, &error).await,
    }
}

fn vault_pull_route<S: SliceWriteStore + Send + Sync + 'static>(
    source: Arc<SliceServerSource<S>>,
    limiter: Arc<RateLimiter>,
) -> Route {
    versioned_route(
        action::SLICE_PULL,
        move |ctx: WsContext, req: SlicePullRequest| {
            let source = source.clone();
            let limiter = limiter.clone();
            async move { handle_vault_pull(&ctx, req, source, limiter.as_ref()).await }
        },
    )
}

async fn handle_vault_pull<S: SliceWriteStore + Send + Sync + 'static>(
    ctx: &WsContext,
    req: SlicePullRequest,
    source: Arc<SliceServerSource<S>>,
    limiter: &RateLimiter,
) -> WsResult<()> {
    let request_id = req.request_id;
    let Some(vault) = require_vault_principal(ctx) else {
        return reply_error(ctx, request_id, &AuthError::MissingToken).await;
    };
    let principal = as_slice_principal(&vault);
    if let Some(reply) = check_rate(ctx, request_id, &principal, limiter) {
        return reply.await;
    }
    if let Err(error) = unbound_operation(ctx) {
        return reply_error(ctx, request_id, &error).await;
    }
    if let Err(error) = admit_if_stored(ctx, &source, &principal, &vault, &req).await {
        return reply_error(ctx, request_id, &error).await;
    }
    let limit = req
        .limit
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .clamp(1, MAX_PAGE_LIMIT);
    match source
        .pull(
            &principal,
            &req.slice,
            req.params,
            req.since,
            req.cursor.as_ref(),
            limit,
        )
        .await
    {
        Ok((records, next_cursor)) => {
            let wire: Vec<WireRecord> = records.iter().map(WireRecord::from).collect();
            let response = match next_cursor {
                Some(cursor) => SlicePullResponse::page(request_id, wire, cursor),
                None => SlicePullResponse::last(request_id, wire),
            };
            ctx.send(response).await
        }
        Err(error) => reply_error(ctx, request_id, &error).await,
    }
}

fn vault_upsert_route<S: SliceWriteStore + Send + Sync + 'static>(
    source: Arc<SliceServerSource<S>>,
    limiter: Arc<RateLimiter>,
    max_payload_bytes: usize,
) -> Route {
    versioned_route(
        action::SLICE_UPSERT,
        move |ctx: WsContext, req: SliceUpsertRequest| {
            let source = source.clone();
            let limiter = limiter.clone();
            async move {
                handle_vault_upsert(&ctx, req, source, limiter.as_ref(), max_payload_bytes).await
            }
        },
    )
}

#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn handle_vault_upsert<S: SliceWriteStore + Send + Sync + 'static>(
    ctx: &WsContext,
    req: SliceUpsertRequest,
    source: Arc<SliceServerSource<S>>,
    limiter: &RateLimiter,
    max_payload_bytes: usize,
) -> WsResult<()> {
    let request_id = req.request_id;
    let Some(vault) = require_vault_principal(ctx) else {
        return reply_error(ctx, request_id, &AuthError::MissingToken).await;
    };
    let principal = as_slice_principal(&vault);
    if let Some(reply) = check_rate(ctx, request_id, &principal, limiter) {
        return reply.await;
    }
    if let Err(error) = unbound_operation(ctx) {
        return reply_error(ctx, request_id, &error).await;
    }
    let record = match SyncRecord::try_from(req.record) {
        Ok(record) => record,
        Err(error) => return reply_error(ctx, request_id, &error).await,
    };
    if let Err(error) = enforce_payload_bound(&record, max_payload_bytes) {
        return reply_error(ctx, request_id, &error).await;
    }
    match source
        .push(&principal, &req.slice, req.params, &record)
        .await
    {
        Ok(()) => {
            if !ctx.is_authenticated() {
                if let Err(error) = ctx.bind_principal(&vault.owner_id().to_string(), vault) {
                    return reply_error(ctx, request_id, &error).await;
                }
                info!("vault owner admitted by its first stored write");
            }
            ctx.send(UpsertAck::applied(request_id)).await
        }
        Err(error) => reply_error(ctx, request_id, &error).await,
    }
}

/// Counts one store operation by a connection that is proven but not yet admitted.
///
/// Past a handful it is refused, so a minted key cannot stream reads and writes.
///
/// # Errors
/// [`WsError::NotAdmitted`] once the connection has used its operations.
fn unbound_operation(ctx: &WsContext) -> WsResult<()> {
    if ctx.is_authenticated() {
        return Ok(());
    }
    match ctx.state::<VaultHandshake>() {
        Some(handshake) if handshake.count_unbound_operation() => Ok(()),
        _ => Err(WsError::NotAdmitted),
    }
}

/// Binds a proven owner to the connection's slot if the store already holds a row under it
/// in the slice being pulled.
///
/// # Errors
/// What [`WsContext::bind_principal`] returns when the owner is stored but no authenticated
/// slot is free.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn admit_if_stored<S: SliceWriteStore + Send + Sync + 'static>(
    ctx: &WsContext,
    source: &SliceServerSource<S>,
    principal: &AuthenticatedPrincipal,
    vault: &VaultPrincipal,
    req: &SlicePullRequest,
) -> WsResult<()> {
    if ctx.is_authenticated() {
        return Ok(());
    }
    let Some(handshake) = ctx.state::<VaultHandshake>() else {
        return Ok(());
    };
    if handshake.probed(&req.slice) {
        return Ok(());
    }
    match source
        .pull(principal, &req.slice, req.params.clone(), 0, None, 1)
        .await
    {
        Ok((records, _)) => {
            handshake.record_probe(&req.slice);
            if records.is_empty() {
                debug!("vault owner holds nothing in this slice; served anonymously");
            } else {
                ctx.bind_principal(&vault.owner_id().to_string(), *vault)?;
                info!("vault owner admitted: the store holds its vault");
            }
        }
        // Not spent: a failed read proves nothing about the owner.
        Err(error) => debug!(
            code = error.error_code(),
            "vault admission probe failed; the owner stays anonymous"
        ),
    }
    Ok(())
}

fn require_vault_principal(ctx: &WsContext) -> Option<VaultPrincipal> {
    ctx.state::<VaultHandshake>()
        .and_then(VaultHandshake::principal)
        .cloned()
}

/// Bridges a [`VaultPrincipal`] into an [`AuthenticatedPrincipal`] whose scope is the owner id.
fn as_slice_principal(vault: &VaultPrincipal) -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        user_id: UserId::new(vault.owner_id().to_string()),
        device_id: None,
        scopes: Vec::new(),
        expires_at: i64::MAX,
    }
}

/// Rejects a sealed row whose total payload exceeds `max_payload_bytes`.
fn enforce_payload_bound(record: &SyncRecord, max_payload_bytes: usize) -> Result<(), VaultError> {
    let bytes: usize = record.payload.values().map(String::len).sum();
    if bytes > max_payload_bytes {
        return Err(VaultError::CiphertextTooLarge {
            bytes,
            max: max_payload_bytes,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::secrets::Secret;
    use fundamental_keys::VaultOwnerKey;
    use fundamental_sync::SyncMeta;
    use std::collections::BTreeMap;

    fn record(ciphertext: &str) -> SyncRecord {
        let mut payload = BTreeMap::new();
        payload.insert("owner_id".to_string(), "abcd".to_string());
        payload.insert("ciphertext".to_string(), ciphertext.to_string());
        SyncRecord::new("row-1", SyncMeta::new(1, 1), payload)
    }

    #[test]
    fn the_owner_id_occupies_the_slice_scope() {
        let key = VaultOwnerKey::from_child_seed(&Secret::new([0x21; 32]));
        let principal = as_slice_principal(&VaultPrincipal::new(key.owner_id()));
        assert_eq!(principal.user_id.as_str(), key.owner_id().to_string());
        assert!(principal.scopes.is_empty());
        assert!(principal.device_id.is_none());
    }

    #[test]
    fn a_payload_within_the_bound_is_accepted() {
        assert!(enforce_payload_bound(&record("small"), 1024).is_ok());
    }

    #[test]
    fn an_oversized_payload_is_rejected() {
        let big = "x".repeat(2048);
        assert!(matches!(
            enforce_payload_bound(&record(&big), 1024),
            Err(VaultError::CiphertextTooLarge { max: 1024, .. })
        ));
    }
}
