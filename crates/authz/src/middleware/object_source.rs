//! [`ObjectSource`]: where a [`PermissionGuard`](super::PermissionGuard) finds the object it
//! authorizes against.

use fundamental_server::HttpContext;

/// Resolves the id of the object a guarded request is about.
///
/// `None` is a routing mistake, not a denial
/// ([`Forbidden::MissingObject`](super::Forbidden::MissingObject)).
pub trait ObjectSource<S>: Send + Sync + 'static {
    /// Whether the id comes from the request path.
    ///
    /// Lets the server check at startup that a route with a path parameter authorizes from it;
    /// see [`Middleware::AUTHORIZES_FROM_PATH`](fundamental_server::Middleware::AUTHORIZES_FROM_PATH).
    const FROM_PATH: bool = false;

    /// The object id for this request, if one can be resolved.
    fn object_id(&self, ctx: &HttpContext<S>) -> Option<String>;

    /// How this source is addressed, for the diagnostic when it resolves nothing.
    fn describe(&self) -> String;
}

/// The object id comes from a path parameter: the named one, or the sole one.
///
/// The default.
#[derive(Debug, Default)]
pub struct PathObject {
    /// The parameter to read; `None` means the sole path parameter.
    pub(super) param: Option<String>,
}

impl<S> ObjectSource<S> for PathObject
where
    S: Send + Sync + 'static,
{
    const FROM_PATH: bool = true;

    fn object_id(&self, ctx: &HttpContext<S>) -> Option<String> {
        match &self.param {
            Some(name) => ctx.params().get(name).map(str::to_string),
            None => {
                let params = ctx.params();
                (params.len() == 1)
                    .then(|| params.iter().next().map(|(_, value)| value.to_string()))
                    .flatten()
            }
        }
    }

    fn describe(&self) -> String {
        match &self.param {
            Some(name) => format!("path parameter `{name}`"),
            None => "the route's sole path parameter".to_string(),
        }
    }
}

/// The object id is fixed for the route.
#[derive(Debug)]
pub struct ConstObject(pub(super) String);

impl<S> ObjectSource<S> for ConstObject
where
    S: Send + Sync + 'static,
{
    fn object_id(&self, _ctx: &HttpContext<S>) -> Option<String> {
        Some(self.0.clone())
    }

    fn describe(&self) -> String {
        format!("the constant `{}`", self.0)
    }
}

/// The object id is derived from the app state — a tenant, an issuer, whatever this
/// deployment is, while the route stays a static declaration.
pub struct StateObject<F>(pub(super) F);

impl<S, F> ObjectSource<S> for StateObject<F>
where
    S: Send + Sync + 'static,
    F: Fn(&S) -> Option<String> + Send + Sync + 'static,
{
    fn object_id(&self, ctx: &HttpContext<S>) -> Option<String> {
        (self.0)(ctx.state())
    }

    fn describe(&self) -> String {
        "the app state".to_string()
    }
}
