use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use fundamental_auth::{AuthError, UntrustedToken};
use fundamental_keys::{DeviceId, VerifyingKey};
use fundamental_sync::wire::{DeviceAck, EnrollRequest, RevokeRequest, RotateRequest, action};
use fundamental_ws::{Route, WsContext, WsResult};

use crate::EnrollmentRegistry;
use crate::sync_endpoint::{reply_error, require_principal, versioned_route};

/// The grace window a rotated key keeps verifying for: five minutes.
const GRACE_MS: i64 = 300_000;

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

pub(crate) fn enroll_route(registry: Arc<EnrollmentRegistry>) -> Route {
    versioned_route(
        action::DEVICE_ENROLL,
        move |ctx: WsContext, req: EnrollRequest| {
            let registry = registry.clone();
            async move { handle_enroll(&ctx, req, registry.as_ref()).await }
        },
    )
}

pub(crate) fn rotate_route(registry: Arc<EnrollmentRegistry>) -> Route {
    versioned_route(
        action::DEVICE_ROTATE,
        move |ctx: WsContext, req: RotateRequest| {
            let registry = registry.clone();
            async move { handle_rotate(&ctx, req, registry.as_ref()).await }
        },
    )
}

pub(crate) fn revoke_route(registry: Arc<EnrollmentRegistry>) -> Route {
    versioned_route(
        action::DEVICE_REVOKE,
        move |ctx: WsContext, req: RevokeRequest| {
            let registry = registry.clone();
            async move { handle_revoke(&ctx, req, registry.as_ref()).await }
        },
    )
}

async fn handle_enroll(
    ctx: &WsContext,
    req: EnrollRequest,
    registry: &EnrollmentRegistry,
) -> WsResult<()> {
    let request_id = req.request_id;
    let Some(principal) = require_principal(ctx) else {
        return reply_error(ctx, request_id, &AuthError::MissingToken).await;
    };
    let public_key = match req.public_key.parse::<VerifyingKey>() {
        Ok(key) => key,
        Err(error) => return reply_error(ctx, request_id, &error).await,
    };
    let device_id = DeviceId::new(req.device_id);
    match registry
        .enroll(principal.user_id, device_id, public_key, now_ms())
        .await
    {
        Ok(()) => ctx.send(DeviceAck::applied(request_id)).await,
        Err(error) => reply_error(ctx, request_id, &error).await,
    }
}

async fn handle_rotate(
    ctx: &WsContext,
    req: RotateRequest,
    registry: &EnrollmentRegistry,
) -> WsResult<()> {
    let request_id = req.request_id;
    let Some(principal) = require_principal(ctx) else {
        return reply_error(ctx, request_id, &AuthError::MissingToken).await;
    };
    let device_id = DeviceId::new(req.device_id);
    match registry.owner(&device_id).await {
        Ok(owner) if owner.as_ref() == Some(&principal.user_id) => {}
        Ok(_) => {
            return reply_error(
                ctx,
                request_id,
                &AuthError::Untrusted(UntrustedToken::DeviceNotOwned),
            )
            .await;
        }
        Err(error) => return reply_error(ctx, request_id, &error).await,
    }
    let new_key = match req.new_public_key.parse::<VerifyingKey>() {
        Ok(key) => key,
        Err(error) => return reply_error(ctx, request_id, &error).await,
    };
    match registry
        .rotate(&device_id, new_key, now_ms(), GRACE_MS)
        .await
    {
        Ok(()) => ctx.send(DeviceAck::applied(request_id)).await,
        Err(error) => reply_error(ctx, request_id, &error).await,
    }
}

async fn handle_revoke(
    ctx: &WsContext,
    req: RevokeRequest,
    registry: &EnrollmentRegistry,
) -> WsResult<()> {
    let request_id = req.request_id;
    let Some(principal) = require_principal(ctx) else {
        return reply_error(ctx, request_id, &AuthError::MissingToken).await;
    };
    let device_id = DeviceId::new(req.device_id);
    match registry.owner(&device_id).await {
        Ok(owner) if owner.as_ref() == Some(&principal.user_id) => {}
        Ok(_) => {
            return reply_error(
                ctx,
                request_id,
                &AuthError::Untrusted(UntrustedToken::DeviceNotOwned),
            )
            .await;
        }
        Err(error) => return reply_error(ctx, request_id, &error).await,
    }
    match registry.revoke(&device_id, now_ms()).await {
        Ok(()) => ctx.send(DeviceAck::applied(request_id)).await,
        Err(error) => reply_error(ctx, request_id, &error).await,
    }
}
