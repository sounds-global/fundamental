use std::future::Future;
use std::sync::Arc;

use fundamental_auth::{AuthenticatedPrincipal, PrincipalVerifier};
use fundamental_core::Error;
use fundamental_core::secrets::SecretString;
use fundamental_sync::SyncError;
use fundamental_sync::wire::{AuthAck, AuthRequest, ErrorReply, VersionedRequest, action};
use fundamental_ws::{Route, WsContext, WsError, WsResult};
use serde::de::DeserializeOwned;
use tracing::{info, warn};

use crate::RateLimiter;

/// Binds `action` to `handler`, refusing any frame whose protocol version this
/// build does not serve before the handler sees it.
///
/// Every inbound sync action goes through this, so a device a version behind is refused before
/// any principal, rate or signature check. The refusal is a correlated [`ErrorReply`]: an
/// uncorrelated one would leave the request hanging.
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
pub(crate) fn versioned_route<P, H, Fut>(action: impl Into<String>, handler: H) -> Route
where
    P: VersionedRequest + DeserializeOwned + Send + 'static,
    H: Fn(WsContext, P) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = WsResult<()>> + Send + 'static,
{
    let action = action.into();
    let name = action.clone();
    Route::new(action, move |ctx: WsContext, req: P| {
        let name = name.clone();
        let gated = if req.version_is_supported() {
            Ok(handler(ctx.clone(), req))
        } else {
            Err((
                req.request_id(),
                SyncError::unsupported_protocol_version(req.protocol_version()),
            ))
        };
        async move {
            match gated {
                Ok(run) => run.await,
                Err((request_id, error)) => {
                    if refusal_logged(&ctx, &error) {
                        warn!(
                            action = %name,
                            code = error.error_code(),
                            "refused a frame speaking an unsupported protocol version"
                        );
                    }
                    send_error(&ctx, request_id, &error).await
                }
            }
        }
    })
}

/// Builds the `auth` route: it verifies the first frame's token and binds the
/// resulting principal to the connection, which is what admits it.
pub(crate) fn auth_route<V>(verifier: Arc<V>) -> Route
where
    V: PrincipalVerifier + Send + Sync + 'static,
{
    versioned_route(action::AUTH, move |ctx: WsContext, req: AuthRequest| {
        let verifier = verifier.clone();
        async move { handle_auth(&ctx, req, verifier.as_ref()).await }
    })
}

#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn handle_auth<V: PrincipalVerifier>(
    ctx: &WsContext,
    req: AuthRequest,
    verifier: &V,
) -> WsResult<()> {
    let request_id = req.request_id;
    // Before the token is verified, so re-authenticating buys no verifications.
    if ctx.is_authenticated() {
        return reply_error(ctx, request_id, &WsError::AlreadyAuthenticated).await;
    }
    let token = SecretString::new(req.token);
    match verifier.verify(&token).await {
        Ok(principal) => {
            let holder = principal.user_id.as_str().to_owned();
            match ctx.bind_principal(&holder, principal) {
                Ok(()) => {
                    info!("connection authenticated");
                    ctx.send(AuthAck::authenticated(request_id)).await
                }
                Err(error) => reply_error(ctx, request_id, &error).await,
            }
        }
        Err(error) => {
            if refusal_logged(ctx, &error) {
                warn!(code = error.error_code(), "authentication rejected");
            }
            send_error(ctx, request_id, &error).await
        }
    }
}

pub(crate) fn require_principal(ctx: &WsContext) -> Option<AuthenticatedPrincipal> {
    ctx.principal::<AuthenticatedPrincipal>().cloned()
}

/// Checks the rate limit, returning a pending error reply if the principal is throttled.
pub(crate) fn check_rate<'a>(
    ctx: &'a WsContext,
    request_id: u64,
    principal: &AuthenticatedPrincipal,
    limiter: &RateLimiter,
) -> Option<impl std::future::Future<Output = WsResult<()>> + 'a> {
    match limiter.check(principal.user_id.as_str()) {
        Ok(()) => None,
        Err(retry) => {
            if ctx.count_refusal() {
                warn!(
                    user_id = principal.user_id.as_str(),
                    retry_ms = retry.as_millis() as u64,
                    "principal rate limited"
                );
            }
            let message = format!("rate limited; retry in {}ms", retry.as_millis());
            Some(async move {
                ctx.send(ErrorReply::new(request_id, "SYNC_429", message))
                    .await
            })
        }
    }
}

/// Sends a correlated error reply, counting a client-caused refusal against the connection's
/// allowance.
pub(crate) async fn reply_error<E: Error>(
    ctx: &WsContext,
    request_id: u64,
    error: &E,
) -> WsResult<()> {
    if error.http_status_code() < 500 && ctx.count_refusal() {
        warn!(code = error.error_code(), "request refused");
    }
    send_error(ctx, request_id, error).await
}

/// Counts a client-caused `error` as a refusal, returning whether a warning may be written.
pub(crate) fn refusal_logged<E: Error>(ctx: &WsContext, error: &E) -> bool {
    error.http_status_code() >= 500 || ctx.count_refusal()
}

/// Sends a correlated error reply without counting it; the caller already has.
async fn send_error<E: Error>(ctx: &WsContext, request_id: u64, error: &E) -> WsResult<()> {
    ctx.send(ErrorReply::new(
        request_id,
        error.error_code(),
        error.display_message(),
    ))
    .await
}
