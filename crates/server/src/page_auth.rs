use std::sync::Arc;

use fundamental_core::AnyError;
use fundamental_core::Error as _;
use tracing::{debug, warn};

use crate::{
    Audience, HttpContext, LoginRoute, Middleware, MiddlewareResult, Next, PageVerdict,
    PublicRoute, Redirect, ServerError, request_return_path,
};

/// The deny-by-default page gate: an anonymous navigation to a gated page is sent to sign in.
///
/// Everything is gated but an explicit public list, so a new page file inherits protection.
/// It is not `needs_principal`, and it answers "who are you", never "may you".
#[derive(Clone, Debug)]
pub struct PageAuth {
    login: LoginRoute,
    public: Arc<Vec<PublicRoute>>,
    /// Whether every request reaching this gate is known to be a navigation.
    every_caller_navigates: bool,
}

impl PageAuth {
    /// A gate that sends anonymous navigations to `login`, itself public, and serves `public` without a principal.
    #[must_use]
    pub fn new(login: LoginRoute, public: &[PublicRoute]) -> Self {
        let mut routes = Vec::with_capacity(public.len() + 1);
        routes.push(login.as_public());
        routes.extend_from_slice(public);
        Self {
            login,
            public: Arc::new(routes),
            every_caller_navigates: false,
        }
    }

    /// The same gate, mounted where every request is a top-level navigation: a native app's page bridge.
    #[must_use]
    pub fn for_navigations(login: LoginRoute, public: &[PublicRoute]) -> Self {
        Self {
            every_caller_navigates: true,
            ..Self::new(login, public)
        }
    }

    /// Decide one request; a pure function of the context.
    #[must_use]
    pub fn verdict<S>(&self, context: &HttpContext<S>) -> PageVerdict {
        if context.principal_opt().is_some() {
            return PageVerdict::Allow;
        }
        // No matched template fails closed.
        let template = context.route_template().unwrap_or_default();
        if self.public.iter().any(|route| route.matches(template)) {
            return PageVerdict::Allow;
        }
        let audience = if self.every_caller_navigates {
            Audience::Navigation
        } else {
            Audience::of(context)
        };
        match audience {
            Audience::Navigation => PageVerdict::Redirect(request_return_path(context)),
            Audience::Programmatic => PageVerdict::Refuse,
        }
    }

    /// The sign-in route this gate redirects to.
    #[must_use]
    pub const fn login(&self) -> LoginRoute {
        self.login
    }
}

/// Gates every route on the router it is mounted on.
impl<S> Middleware<S> for PageAuth
where
    S: Send + Sync + 'static,
{
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    async fn handle(
        &self,
        ctx: HttpContext<S>,
        body: Vec<u8>,
        next: Next<'_, S>,
    ) -> MiddlewareResult {
        let template = ctx.route_template().unwrap_or("<unmatched>").to_owned();
        match self.verdict(&ctx) {
            PageVerdict::Allow => next.run(ctx, body).await,
            PageVerdict::Redirect(return_to) => {
                // The route template, never the rebuilt path. `303`, so a `POST` with an expired session becomes a `GET`.
                let location = self.login.location(return_to.as_ref(), ctx.address());
                let redirect = match Redirect::see_other(location) {
                    Ok(redirect) => redirect,
                    Err(error) => {
                        // A door that cannot be written is a refusal, never the page it was guarding.
                        warn!(route = %template, error = error.error_code(), outcome = "refused", "page auth: the sign-in location cannot be sent");
                        return Err(AnyError::new(ServerError::unauthenticated()));
                    }
                };
                debug!(route = %template, outcome = "redirected", "page auth");
                ctx.redirect(redirect);
                // A cached redirect would keep a person signed out after signing in.
                ctx.set_response_header("cache-control", "no-store");
                ctx.set_response_header("vary", "cookie, sec-fetch-mode");
                Ok(Vec::new())
            }
            PageVerdict::Refuse => {
                warn!(route = %template, outcome = "refused", "page auth");
                Err(AnyError::new(ServerError::unauthenticated()))
            }
        }
    }
}
