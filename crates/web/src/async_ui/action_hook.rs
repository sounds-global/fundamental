use super::{ActionQuerySource, At, BlockingSource, DeclaredPath, LocalSource, NoQuerySource};
use core::marker::PhantomData;
use fundamental_core::action::ActionPath;

/// A generated action hook consumed by [`Query`](super::Query) or [`Mutation`](super::Mutation).
pub struct ActionHook<M, O, S = NoQuerySource, P = DeclaredPath> {
    pub(super) source: S,
    pub(super) path: P,
    marker: PhantomData<fn() -> (M, O)>,
}

impl<M, O> ActionHook<M, O, NoQuerySource> {
    /// Creates an unconfigured handle for a mounted action.
    #[doc(hidden)]
    pub const fn new() -> Self {
        Self {
            source: NoQuerySource,
            path: DeclaredPath,
            marker: PhantomData,
        }
    }
}

impl<M, O, S, P> ActionHook<M, O, S, P> {
    fn with_source<T>(self, source: impl FnOnce(S) -> T) -> ActionHook<M, O, T, P> {
        ActionHook {
            source: source(self.source),
            path: self.path,
            marker: PhantomData,
        }
    }
}

impl<M, O, S> ActionHook<M, O, S, DeclaredPath> {
    /// Calls the action at the path `path` fills, needed when the action's path holds captures.
    pub fn at<F>(self, path: F) -> ActionHook<M, O, S, At<F>>
    where
        F: Fn() -> ActionPath<M>,
    {
        ActionHook {
            source: self.source,
            path: At(path),
            marker: PhantomData,
        }
    }
}

impl<M, O, S, P> ActionHook<M, O, S, P> {
    /// Uses a browser-local query resource or local Leptos mutation action.
    pub fn local(self) -> ActionHook<M, O, LocalSource<S>, P> {
        self.with_source(LocalSource)
    }
}

impl<M, O, P> ActionHook<M, O, NoQuerySource, P> {
    /// Sets a reactive input source for query actions with an input value.
    pub fn source<F>(self, source: F) -> ActionHook<M, O, ActionQuerySource<F>, P> {
        self.with_source(|_| ActionQuerySource(source))
    }
}

impl<M, O, P> ActionHook<M, O, LocalSource<NoQuerySource>, P> {
    /// Sets a reactive input source while retaining browser-local execution.
    pub fn source<F>(self, source: F) -> ActionHook<M, O, LocalSource<ActionQuerySource<F>>, P> {
        self.with_source(|_| LocalSource(ActionQuerySource(source)))
    }
}

impl<M, O, P> ActionHook<M, O, BlockingSource<NoQuerySource>, P> {
    /// Sets a reactive input source while the query still holds its page.
    pub fn source<F>(self, source: F) -> ActionHook<M, O, BlockingSource<ActionQuerySource<F>>, P> {
        self.with_source(|_| BlockingSource(ActionQuerySource(source)))
    }
}

impl<M, O, P> ActionHook<M, O, NoQuerySource, P> {
    /// Holds the page's response until this server query resolves.
    ///
    /// A blocking query that fails answers the whole page as a loader's refusal does.
    ///
    /// ```rust
    /// use fundamental_core::AnyError;
    /// use fundamental_macros::{action, mount_actions};
    /// use fundamental_web::PageContext;
    /// use fundamental_web::prelude::*;
    ///
    /// #[action(GET, "/api/project", reactive(query))]
    /// async fn project(_ctx: fundamental_web::HttpContext<()>) -> Result<String, AnyError> {
    ///     Ok("Held project".to_owned())
    /// }
    ///
    /// mount_actions! { Project }
    ///
    /// #[component]
    /// fn ProjectPage() -> impl IntoView {
    ///     // The page answers once the project is read, named after it.
    ///     let project = actions::use_project().blocking();
    ///     view! {
    ///         <Query resource=project let:name>
    ///             {expect_context::<PageContext>().set_title(name.clone())}
    ///             <h1>{name}</h1>
    ///         </Query>
    ///     }
    /// }
    /// # fn main() {}
    /// ```
    pub fn blocking(self) -> ActionHook<M, O, BlockingSource<NoQuerySource>, P> {
        self.with_source(BlockingSource)
    }
}

impl<M, O, F, P> ActionHook<M, O, ActionQuerySource<F>, P> {
    /// Holds the page's response until this server query resolves; see [`blocking`](ActionHook::blocking).
    pub fn blocking(self) -> ActionHook<M, O, BlockingSource<ActionQuerySource<F>>, P> {
        self.with_source(BlockingSource)
    }
}

impl<M, O, S, P> Copy for ActionHook<M, O, S, P>
where
    S: Copy,
    P: Copy,
{
}

impl<M, O, S, P> Clone for ActionHook<M, O, S, P>
where
    S: Clone,
    P: Clone,
{
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            path: self.path.clone(),
            marker: PhantomData,
        }
    }
}
