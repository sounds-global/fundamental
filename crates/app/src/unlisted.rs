use fundamental_server::{HttpContext, Middleware, MiddlewareResult, Next};

/// Answers a page under one of the app's unlisted paths with `X-Robots-Tag: noindex`.
#[derive(Debug, Clone)]
pub(crate) struct Unlisted {
    prefixes: Vec<&'static str>,
}

impl Unlisted {
    /// Unlists every page at or under each of `prefixes`.
    pub(crate) fn new(prefixes: Vec<&'static str>) -> Self {
        Self { prefixes }
    }

    /// Whether the page at the canonical `path` is unlisted.
    pub(crate) fn covers(&self, path: &str) -> bool {
        self.prefixes.iter().any(|prefix| covers(prefix, path))
    }
}

/// Whether `path` is `prefix` or below it, segment by segment.
fn covers(prefix: &str, path: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    prefix.is_empty()
        || path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

impl<S> Middleware<S> for Unlisted
where
    S: Send + Sync + 'static,
{
    async fn handle(
        &self,
        ctx: HttpContext<S>,
        body: Vec<u8>,
        next: Next<'_, S>,
    ) -> MiddlewareResult {
        if ctx
            .route_template()
            .is_some_and(|template| self.covers(template))
        {
            ctx.set_response_header("x-robots-tag", "noindex");
        }
        next.run(ctx, body).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prefix_covers_itself_and_the_pages_below_it_only() {
        let unlisted = Unlisted::new(vec!["/review", "/brand"]);
        assert!(unlisted.covers("/review"));
        assert!(unlisted.covers("/review/article"));
        assert!(unlisted.covers("/brand"));
        assert!(!unlisted.covers("/reviews"));
        assert!(!unlisted.covers("/"));
    }
}
