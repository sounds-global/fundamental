use std::sync::Arc;

use fundamental_auth::{AuthError, PrincipalVerifier};
use fundamental_keys::{DeviceId, KeyError, Signature};
use fundamental_sync::wire::{
    SlicePullRequest, SlicePullResponse, SliceSubscribeAck, SliceSubscribeRequest,
    SliceUpsertRequest, UpsertAck, WireRecord, action, canonical_bytes,
};

const DEFAULT_PAGE_LIMIT: u32 = 500;
/// The largest page a `slice.pull` returns.
const MAX_PAGE_LIMIT: u32 = 1000;
use fundamental_sync::{BroadcastChangePublisher, SliceStore, SliceWriteStore, SyncRecord};
use fundamental_ws::{Route, WsContext, WsResult, WsRouter};
use tracing::info;

use crate::signed_endpoint::{enroll_route, now_ms, revoke_route, rotate_route};
use crate::slice::{SliceSubscriptions, run_fan_out};
use crate::sync_endpoint::{
    auth_route, check_rate, reply_error, require_principal, versioned_route,
};
use crate::{EnrollmentRegistry, RateLimiter, SliceServerSource};

/// Builds the slice sync endpoint router.
///
/// It authenticates on the first `auth` frame and serves `slice.pull` and `slice.upsert` for
/// the bound principal, throttled by `limiter`; a pull-only slice refuses upserts.
pub fn slice_sync_router<V, S>(
    verifier: Arc<V>,
    source: Arc<SliceServerSource<S>>,
    limiter: Arc<RateLimiter>,
) -> WsRouter
where
    V: PrincipalVerifier + Send + Sync + 'static,
    S: SliceWriteStore + Send + Sync + 'static,
{
    WsRouter::new()
        .route(auth_route(verifier))
        .route(slice_pull_route(source.clone(), limiter.clone()))
        .route(slice_upsert_route(source, limiter))
}

/// Builds the slice sync endpoint with **live push** on top of [`slice_sync_router`].
///
/// A connection may `slice.subscribe`; the server then pushes a `slice.change` frame for every
/// in-scope change `publisher` reports.
///
/// Must be called within a Tokio runtime: it spawns the fan-out task, which ends when
/// `publisher` is dropped.
pub fn live_slice_sync_router<V, S>(
    verifier: Arc<V>,
    source: Arc<SliceServerSource<S>>,
    limiter: Arc<RateLimiter>,
    publisher: &BroadcastChangePublisher,
) -> WsRouter
where
    V: PrincipalVerifier + Send + Sync + 'static,
    S: SliceWriteStore + Send + Sync + 'static,
{
    let subscriptions = Arc::new(SliceSubscriptions::new());
    tokio::spawn(run_fan_out(publisher.subscribe(), subscriptions.clone()));
    WsRouter::new()
        .route(auth_route(verifier))
        .route(slice_pull_route(source.clone(), limiter.clone()))
        .route(slice_upsert_route(source.clone(), limiter.clone()))
        .route(slice_subscribe_route(source, limiter, subscriptions))
}

fn slice_subscribe_route<S: SliceStore + Send + Sync + 'static>(
    source: Arc<SliceServerSource<S>>,
    limiter: Arc<RateLimiter>,
    subscriptions: Arc<SliceSubscriptions>,
) -> Route {
    versioned_route(
        action::SLICE_SUBSCRIBE,
        move |ctx: WsContext, req: SliceSubscribeRequest| {
            let source = source.clone();
            let limiter = limiter.clone();
            let subscriptions = subscriptions.clone();
            async move {
                handle_slice_subscribe(&ctx, req, source, limiter.as_ref(), subscriptions.as_ref())
                    .await
            }
        },
    )
}

async fn handle_slice_subscribe<S: SliceStore + Send + Sync + 'static>(
    ctx: &WsContext,
    req: SliceSubscribeRequest,
    source: Arc<SliceServerSource<S>>,
    limiter: &RateLimiter,
    subscriptions: &SliceSubscriptions,
) -> WsResult<()> {
    let request_id = req.request_id;
    let Some(principal) = require_principal(ctx) else {
        return reply_error(ctx, request_id, &AuthError::MissingToken).await;
    };
    if let Some(reply) = check_rate(ctx, request_id, &principal, limiter) {
        return reply.await;
    }
    match source.resolve(&principal, &req.slice, req.params).await {
        Ok(resolved) => {
            subscriptions.subscribe(
                principal.user_id.as_str().to_string(),
                ctx.connection_id().clone(),
                ctx.clone(),
                resolved.table.to_string(),
                resolved.key_columns,
                resolved.filter,
            );
            info!(slice = %resolved.name, "slice subscription registered");
            ctx.send(SliceSubscribeAck::new(request_id, req.slice))
                .await
        }
        Err(error) => reply_error(ctx, request_id, &error).await,
    }
}

fn slice_pull_route<S: SliceStore + Send + Sync + 'static>(
    source: Arc<SliceServerSource<S>>,
    limiter: Arc<RateLimiter>,
) -> Route {
    versioned_route(
        action::SLICE_PULL,
        move |ctx: WsContext, req: SlicePullRequest| {
            let source = source.clone();
            let limiter = limiter.clone();
            async move { handle_slice_pull(&ctx, req, source, limiter.as_ref()).await }
        },
    )
}

async fn handle_slice_pull<S: SliceStore + Send + Sync + 'static>(
    ctx: &WsContext,
    req: SlicePullRequest,
    source: Arc<SliceServerSource<S>>,
    limiter: &RateLimiter,
) -> WsResult<()> {
    let request_id = req.request_id;
    let Some(principal) = require_principal(ctx) else {
        return reply_error(ctx, request_id, &AuthError::MissingToken).await;
    };
    if let Some(reply) = check_rate(ctx, request_id, &principal, limiter) {
        return reply.await;
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

fn slice_upsert_route<S: SliceWriteStore + Send + Sync + 'static>(
    source: Arc<SliceServerSource<S>>,
    limiter: Arc<RateLimiter>,
) -> Route {
    versioned_route(
        action::SLICE_UPSERT,
        move |ctx: WsContext, req: SliceUpsertRequest| {
            let source = source.clone();
            let limiter = limiter.clone();
            async move { handle_slice_upsert(&ctx, req, source, limiter.as_ref()).await }
        },
    )
}

async fn handle_slice_upsert<S: SliceWriteStore + Send + Sync + 'static>(
    ctx: &WsContext,
    req: SliceUpsertRequest,
    source: Arc<SliceServerSource<S>>,
    limiter: &RateLimiter,
) -> WsResult<()> {
    let request_id = req.request_id;
    let Some(principal) = require_principal(ctx) else {
        return reply_error(ctx, request_id, &AuthError::MissingToken).await;
    };
    if let Some(reply) = check_rate(ctx, request_id, &principal, limiter) {
        return reply.await;
    }
    let record = match SyncRecord::try_from(req.record) {
        Ok(record) => record,
        Err(error) => return reply_error(ctx, request_id, &error).await,
    };
    match source
        .push(&principal, &req.slice, req.params, &record)
        .await
    {
        Ok(()) => ctx.send(UpsertAck::applied(request_id)).await,
        Err(error) => reply_error(ctx, request_id, &error).await,
    }
}

/// Builds the device-signed slice endpoint router.
///
/// Every `slice.upsert` must carry a `device_id` and a signature over
/// `canonical_bytes(slice, record)` from the device's enrolled key; the device lifecycle routes
/// are served too.
pub fn signed_slice_sync_router<V, S>(
    verifier: Arc<V>,
    source: Arc<SliceServerSource<S>>,
    limiter: Arc<RateLimiter>,
    registry: Arc<EnrollmentRegistry>,
) -> WsRouter
where
    V: PrincipalVerifier + Send + Sync + 'static,
    S: SliceWriteStore + Send + Sync + 'static,
{
    WsRouter::new()
        .route(auth_route(verifier))
        .route(slice_pull_route(source.clone(), limiter.clone()))
        .route(signed_slice_upsert_route(source, limiter, registry.clone()))
        .route(enroll_route(registry.clone()))
        .route(rotate_route(registry.clone()))
        .route(revoke_route(registry))
}

fn signed_slice_upsert_route<S: SliceWriteStore + Send + Sync + 'static>(
    source: Arc<SliceServerSource<S>>,
    limiter: Arc<RateLimiter>,
    registry: Arc<EnrollmentRegistry>,
) -> Route {
    versioned_route(
        action::SLICE_UPSERT,
        move |ctx: WsContext, req: SliceUpsertRequest| {
            let source = source.clone();
            let limiter = limiter.clone();
            let registry = registry.clone();
            async move {
                handle_signed_slice_upsert(&ctx, req, source, limiter.as_ref(), registry.as_ref())
                    .await
            }
        },
    )
}

#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn handle_signed_slice_upsert<S: SliceWriteStore + Send + Sync + 'static>(
    ctx: &WsContext,
    req: SliceUpsertRequest,
    source: Arc<SliceServerSource<S>>,
    limiter: &RateLimiter,
    registry: &EnrollmentRegistry,
) -> WsResult<()> {
    let request_id = req.request_id;
    let Some(principal) = require_principal(ctx) else {
        return reply_error(ctx, request_id, &AuthError::MissingToken).await;
    };
    if let Some(reply) = check_rate(ctx, request_id, &principal, limiter) {
        return reply.await;
    }
    if req.device_id.is_empty() || req.signature.is_empty() {
        return reply_error(ctx, request_id, &KeyError::BadSignature).await;
    }
    let device_id = DeviceId::new(req.device_id.clone());
    let signature = match req.signature.parse::<Signature>() {
        Ok(signature) => signature,
        Err(error) => return reply_error(ctx, request_id, &error).await,
    };
    let record = match SyncRecord::try_from(req.record) {
        Ok(record) => record,
        Err(error) => return reply_error(ctx, request_id, &error).await,
    };
    let canonical = canonical_bytes(&req.slice, &record);
    let device_key = match registry
        .verify_push(
            &device_id,
            &principal.user_id,
            &canonical,
            &signature,
            now_ms(),
        )
        .await
    {
        Ok(device_key) => device_key,
        Err(error) => return reply_error(ctx, request_id, &error).await,
    };
    info!(device_key = %device_key, "device push signature verified");
    match source
        .push(&principal, &req.slice, req.params, &record)
        .await
    {
        Ok(()) => ctx.send(UpsertAck::applied(request_id)).await,
        Err(error) => reply_error(ctx, request_id, &error).await,
    }
}
