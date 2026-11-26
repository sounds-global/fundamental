use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Instant;

use fundamental_core::{AnyError, Error as _};
use fundamental_server::{HttpContext, Method, Middleware, Route};
use leptos::prelude::*;
use tracing::{Instrument, error, field, info_span};

use super::gate_reason::GateReason;
use super::guard_state::GuardState;
use super::load_cx::LoadCx;
use super::loads::Loads;
use super::open::Open;
use super::page_policy_error::PagePolicyError;
use super::page_policy_guard::PagePolicyGuard;
use super::policy_chain::PolicyChain;
use super::refused::Refused;
use super::route_policy::RoutePolicy;
use crate::in_request_language::in_request_language;
use crate::page_context::PageContext;
use crate::page_data::PageData;
use crate::render_options::RenderOptions;
use crate::render_page_stream::render_page_stream_with_options as render_page_stream;

/// The route template the gate carries: not an address, so no request path can reach it.
const GATE_LABEL: &str = "__gate";

/// One page route as the generated registration builds it: guards, then loaders, then the render.
#[must_use]
pub struct PageRoute<G = Open> {
    policy: Arc<RoutePolicy>,
    state: PhantomData<fn() -> G>,
}

impl PageRoute<Open> {
    /// The route `template`, under the policies of `chain`.
    ///
    /// # Errors
    /// [`PagePolicyError`] (`FW_PAGE_002`–`005`) when the chain cannot be served on this
    /// route.
    pub fn new<G: GuardState>(
        template: &str,
        chain: PolicyChain<G>,
    ) -> Result<PageRoute<G>, AnyError> {
        let policy = RoutePolicy::compose(template, chain).map_err(refused_at_boot)?;
        Ok(PageRoute {
            policy: Arc::new(policy),
            state: PhantomData,
        })
    }

    /// The gate: the page a navigation refused with `401` or `403` renders, with no application state.
    pub fn gate<S, F, V, D>(view: F, document: D) -> Route<S>
    where
        S: Clone + Send + Sync + 'static,
        F: Fn(Refused) -> V + Send + Sync + 'static,
        V: RenderHtml + 'static,
        D: Fn(&PageContext, &str) -> String + Copy + Send + Sync + 'static,
    {
        let view = Arc::new(view);
        Route::streaming_page(Method::Get, GATE_LABEL, move |ctx: HttpContext<S>| {
            let view = Arc::clone(&view);
            async move {
                let refused = ctx.shown_error().map_or(
                    Refused::new(403, GateReason::Forbidden),
                    Refused::from_shown,
                );
                let rendered = render_page_stream(
                    &ctx,
                    RenderOptions {
                        without_state: true,
                        title: None,
                    },
                    provide_nothing,
                    move || view(refused),
                    document,
                )
                .await?;
                Ok::<_, AnyError>(rendered)
            }
        })
    }
}

const fn provide_nothing() {}

impl<G: GuardState> PageRoute<G> {
    /// The route serving a page with no loader, the application state in context.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub fn view<S, F, V, D>(self, view: F, document: D) -> Route<S>
    where
        S: Clone + Send + Sync + 'static,
        PagePolicyGuard<G>: Middleware<S>,
        F: Fn() -> V + Send + Sync + 'static,
        V: RenderHtml + 'static,
        D: Fn(&PageContext, &str) -> String + Copy + Send + Sync + 'static,
    {
        let title = self.policy.title.clone();
        let view = Arc::new(view);
        Route::streaming_page(
            Method::Get,
            self.policy.route.clone(),
            move |ctx: HttpContext<S>| {
                let view = Arc::clone(&view);
                let title = title.clone();
                async move {
                    let options = RenderOptions {
                        without_state: false,
                        title,
                    };
                    let rendered =
                        render_page_stream(&ctx, options, || {}, move || view(), document).await?;
                    Ok::<_, AnyError>(rendered)
                }
            },
        )
        .middleware(PagePolicyGuard::<G>::new(self.policy))
    }

    /// The route serving a page whose model and layout frames `loads` loads before `view` renders.
    ///
    /// # Errors
    /// [`PagePolicyError::LoaderOutranksGuards`] (`FW_PAGE_001`) when a loader takes an
    /// object at a relation no guard of the route grants.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub fn loaded<S, A, L, F, V, D>(
        self,
        loads: L,
        view: F,
        document: D,
    ) -> Result<Route<S>, AnyError>
    where
        S: Clone + Send + Sync + 'static,
        PagePolicyGuard<G>: Middleware<S>,
        L: Loads<S, A>,
        F: Fn(L::Models) -> V + Send + Sync + 'static,
        V: RenderHtml + 'static,
        D: Fn(&PageContext, &str) -> String + Copy + Send + Sync + 'static,
    {
        for (file, required) in loads.requirements() {
            self.policy
                .grants(file, required)
                .map_err(refused_at_boot)?;
        }
        let policy = Arc::clone(&self.policy);
        let loads = Arc::new(loads);
        let view = Arc::new(view);
        let route = Route::streaming_page(
            Method::Get,
            self.policy.route.clone(),
            move |ctx: HttpContext<S>| {
                let (policy, loads, view) =
                    (Arc::clone(&policy), Arc::clone(&loads), Arc::clone(&view));
                async move {
                    let span = info_span!(
                        "page.load",
                        route = %policy.route,
                        elapsed_ms = field::Empty,
                        code = field::Empty,
                    );
                    let started = Instant::now();
                    let objects = ctx.authorized_objects().clone();
                    let cx = LoadCx::new(ctx.clone(), Arc::clone(&policy.public));
                    let loaded = in_request_language(&ctx, loads.load(&cx, &objects))
                        .instrument(span.clone())
                        .await;
                    span.record("elapsed_ms", started.elapsed().as_millis());
                    let models = loaded.map_err(|refusal| {
                        let answer = refusal.into_answer(&policy.route);
                        span.record("code", answer.get().error_code());
                        answer
                    })?;
                    let rendered = render_page_stream(
                        &ctx,
                        RenderOptions {
                            without_state: true,
                            title: policy.title.clone(),
                        },
                        || {},
                        move || view(models),
                        document,
                    )
                    .await?;
                    Ok::<_, AnyError>(rendered)
                }
            },
        );
        Ok(route.middleware(PagePolicyGuard::<G>::new(self.policy)))
    }

    /// The route serving a page that exports `Data` ([`PageData`]), behind the route's policies.
    pub fn with_data<Data, S, F, V, D>(self, view: F, document: D) -> Route<S>
    where
        S: Clone + Send + Sync + 'static,
        PagePolicyGuard<G>: Middleware<S>,
        Data: PageData<S>,
        F: Fn() -> V + Send + Sync + 'static,
        V: RenderHtml + 'static,
        D: Fn(&PageContext, &str) -> String + Copy + Send + Sync + 'static,
    {
        let title = self.policy.title.clone();
        let view = Arc::new(view);
        Route::streaming_page(
            Method::Get,
            self.policy.route.clone(),
            move |ctx: HttpContext<S>| {
                let view = Arc::clone(&view);
                let title = title.clone();
                async move {
                    let data = in_request_language(&ctx, Data::load(&ctx)).await?;
                    let options = RenderOptions {
                        without_state: false,
                        title,
                    };
                    let rendered = render_page_stream(
                        &ctx,
                        options,
                        move || provide_context(data),
                        move || view(),
                        document,
                    )
                    .await?;
                    Ok::<_, AnyError>(rendered)
                }
            },
        )
        .middleware(PagePolicyGuard::<G>::new(self.policy))
    }
}

/// Logs a boot refusal once, with its code, and hands it to the registration.
fn refused_at_boot(refusal: PagePolicyError) -> AnyError {
    error!(code = refusal.error_code(), error = %refusal, "a page route was refused at boot");
    AnyError::new(refusal)
}
