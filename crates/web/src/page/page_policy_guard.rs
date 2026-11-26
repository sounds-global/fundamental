use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Instant;

use fundamental_authz::authorizer::{DynAuthorizer, HasAuthorizer};
use fundamental_authz::middleware::{Forbidden, GuardDecision};
use fundamental_core::AnyError;
use fundamental_server::{HttpContext, Middleware, MiddlewareResult, Next};
use tracing::{Instrument, error, field, info_span, warn};

use super::guard_object::GuardObject;
use super::guarded::Guarded;
use super::open::Open;
use super::page_error::PageError;
use super::route_policy::RoutePolicy;

/// How long a reader is asked to wait when the authorizer could not answer.
const AUTHORIZER_RETRY_SECS: u64 = 5;

/// The route middleware every generated page route carries: guards, then the route, then `Cache-Control`.
pub struct PagePolicyGuard<G> {
    policy: Arc<RoutePolicy>,
    state: PhantomData<fn() -> G>,
}

impl<G> PagePolicyGuard<G> {
    pub(crate) fn new(policy: Arc<RoutePolicy>) -> Self {
        Self {
            policy,
            state: PhantomData,
        }
    }
}

impl<S> Middleware<S> for PagePolicyGuard<Open>
where
    S: Send + Sync + 'static,
{
    async fn handle(
        &self,
        ctx: HttpContext<S>,
        body: Vec<u8>,
        next: Next<'_, S>,
    ) -> MiddlewareResult {
        enforce(&self.policy, None, ctx, body, next).await
    }
}

impl<S> Middleware<S> for PagePolicyGuard<Guarded>
where
    S: HasAuthorizer + Send + Sync + 'static,
{
    const GRANTS_AUTHORIZATION: bool = true;
    const AUTHORIZES_FROM_PATH: bool = true;

    async fn handle(
        &self,
        ctx: HttpContext<S>,
        body: Vec<u8>,
        next: Next<'_, S>,
    ) -> MiddlewareResult {
        enforce(&self.policy, Some(authorizer_of::<S>), ctx, body, next).await
    }
}

fn authorizer_of<S: HasAuthorizer>(state: &S) -> &dyn DynAuthorizer {
    state.authorizer()
}

async fn enforce<S>(
    policy: &RoutePolicy,
    authorizer: Option<fn(&S) -> &dyn DynAuthorizer>,
    ctx: HttpContext<S>,
    body: Vec<u8>,
    next: Next<'_, S>,
) -> MiddlewareResult
where
    S: Send + Sync + 'static,
{
    let headers = ctx.response_headers();
    let personal = policy.personal() || ctx.principal_opt().is_some();

    if let Err(refusal) = admit(policy, authorizer, &ctx).await {
        headers.replace("cache-control", "no-store");
        return Err(refusal);
    }

    let answer = next.run(ctx, body).await;
    let cache = match (&answer, personal) {
        (Ok(_), false) => policy.cache.header_value(),
        _ => "no-store".to_owned(),
    };
    headers.replace("cache-control", cache);
    answer
}

/// Whether the request may reach the route; each allow is recorded on the context.
#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "readability baseline"
)]
async fn admit<S>(
    policy: &RoutePolicy,
    authorizer: Option<fn(&S) -> &dyn DynAuthorizer>,
    ctx: &HttpContext<S>,
) -> Result<(), AnyError> {
    let principal = ctx.principal_opt().map(|p| p.user_id.to_string());
    if policy.signed_in && principal.is_none() {
        return Err(AnyError::new(PageError::SignInRequired));
    }
    if policy.guards.is_empty() {
        return Ok(());
    }
    let Some(authorizer) = authorizer else {
        // Unreachable through the type state; fails shut if ever reached.
        error!(route = %policy.route, "a guarded page route has no authorizer; refusing");
        return Err(AnyError::new(PageError::GuardedRouteWithoutAuthorizer {
            route: policy.route.clone(),
        }));
    };
    let authorizer = authorizer(ctx.state());

    for guard in &policy.guards {
        let span = info_span!(
            "page.guard",
            route = %policy.route,
            namespace = guard.check.object_namespace(),
            relation = guard.check.relation(),
            outcome = field::Empty,
            elapsed_ms = field::Empty,
        );
        let started = Instant::now();
        let raw_id = match &guard.object {
            GuardObject::Param(param) => ctx.params().get(param).map(str::to_owned),
            GuardObject::Const(id) => Some(id.clone()),
        };
        let Some(raw_id) = raw_id else {
            // `FW_PAGE_004` refuses this at boot, so the registration bypassed the checks.
            error!(
                route = %policy.route,
                namespace = guard.check.object_namespace(),
                relation = guard.check.relation(),
                "a page guard found no object id; refusing"
            );
            return Err(AnyError::new(Forbidden::MissingObject));
        };
        let decision = guard
            .check
            .decide(authorizer, principal.as_deref(), &raw_id)
            .instrument(span.clone())
            .await;
        span.record("elapsed_ms", started.elapsed().as_millis());
        match decision {
            Ok(GuardDecision::Allowed { object_id }) => {
                span.record("outcome", "allowed");
                ctx.authorized_objects().record(
                    guard.check.object_namespace(),
                    guard.check.relation(),
                    &object_id,
                );
            }
            Ok(denied) => {
                span.record("outcome", "denied");
                return Err(denied
                    .refusal()
                    .unwrap_or_else(|| AnyError::new(PageError::Unavailable)));
            }
            Err(failure) if failure.get().http_status_code() >= 500 => {
                span.record("outcome", "unavailable");
                warn!(
                    route = %policy.route,
                    code = failure.get().error_code(),
                    "the authorizer could not answer; the page is unavailable"
                );
                return Err(AnyError::new(PageError::AuthorizerUnavailable {
                    retry_after_secs: AUTHORIZER_RETRY_SECS,
                    source: failure,
                }));
            }
            Err(refused) => {
                span.record("outcome", "refused");
                return Err(refused);
            }
        }
    }
    Ok(())
}
