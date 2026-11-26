use std::sync::Arc;

use tracing::{Instrument, debug, info_span};

use crate::http_context::HttpContext;
use crate::middleware::MiddlewareResult;
use crate::next::Next;
use crate::route::Route;
use crate::segment_resolver::ErasedSegmentResolver;
use crate::{PathParams, ResolvedSegment, SegmentResolution};

/// The resolvers of one route's declared parameters, run between the router's middleware and the route's.
pub(crate) struct SegmentStage<'a> {
    resolvers: Vec<&'a dyn ErasedSegmentResolver>,
}

impl<'a> SegmentStage<'a> {
    pub(crate) fn new(resolvers: Vec<&'a dyn ErasedSegmentResolver>) -> Self {
        Self { resolvers }
    }

    /// The stage of `route`: the resolvers of its declared captures, in path order.
    pub(crate) fn for_route<S>(
        declared: &'a [Arc<dyn ErasedSegmentResolver>],
        route: &Route<S>,
    ) -> Option<Self>
    where
        S: Send + Sync + 'static,
    {
        if declared.is_empty() || route.skips_segments() {
            return None;
        }
        let resolvers: Vec<&dyn ErasedSegmentResolver> = route
            .pattern()
            .params()
            .filter_map(|param| {
                declared
                    .iter()
                    .find(|resolver| resolver.param() == param)
                    .map(|resolver| resolver.as_ref())
            })
            .collect();
        (!resolvers.is_empty()).then(|| Self::new(resolvers))
    }

    /// Resolves each declared segment, rewrites its parameter, records the outcome, and runs the rest of the chain.
    pub(crate) async fn run<S>(
        self,
        ctx: HttpContext<S>,
        body: Vec<u8>,
        next: Next<'a, S>,
    ) -> MiddlewareResult
    where
        S: Send + Sync + 'static,
    {
        let span = info_span!(
            "server.segments",
            route = ctx.route_template().unwrap_or_default(),
            segments = self.resolvers.len(),
        );
        let params = self.resolve(&ctx).instrument(span).await?;
        next.run(ctx.with_params(params), body).await
    }

    async fn resolve<S>(
        &self,
        ctx: &HttpContext<S>,
    ) -> Result<PathParams, fundamental_core::AnyError> {
        let ledger = ctx.segments();
        let locale = ctx.locale();
        let mut params: Vec<(String, String)> = ctx
            .params()
            .iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        // Every capture is recorded as asked, declared or not.
        for (name, value) in &params {
            ledger.record_requested(name, value);
        }
        let mut resolved: Vec<(&str, ResolvedSegment)> = Vec::new();
        for resolver in &self.resolvers {
            let param = resolver.param();
            let Some(slot) = params.iter().position(|(name, _)| name == param) else {
                continue;
            };
            let raw = params[slot].1.clone();
            let scope = match resolver.scope() {
                None => Some(None),
                Some(parent) => resolved
                    .iter()
                    .find(|(name, _)| *name == parent)
                    .map(|(_, segment)| Some(segment)),
            };
            let resolution = match scope {
                Some(scope) if raw.len() <= ResolvedSegment::MAX_LEN => {
                    resolver.resolve(&raw, scope, locale).await?
                }
                Some(_) => SegmentResolution::Unknown,
                // The parent named nothing the reader may see, so neither can its child.
                None => SegmentResolution::Unknown,
            };
            let outcome = match &resolution {
                SegmentResolution::Canonical(segment) => {
                    params[slot].1 = segment.id().to_owned();
                    resolved.push((param, segment.clone()));
                    "canonical"
                }
                SegmentResolution::Moved(segment) => {
                    params[slot].1 = segment.id().to_owned();
                    resolved.push((param, segment.clone()));
                    "moved"
                }
                SegmentResolution::Unknown => {
                    params[slot].1 = resolver.void_id();
                    "unknown"
                }
            };
            debug!(param, outcome, "resolved a path segment");
            ledger.record(param, resolver.namespace(), resolution);
        }
        Ok(params.into_iter().collect())
    }
}
