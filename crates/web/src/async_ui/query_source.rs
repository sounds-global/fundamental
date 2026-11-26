use super::{ActionHook, ActionQuerySource, BlockingSource, HookPath, LocalSource, NoQuerySource};
#[cfg(feature = "ssr")]
use crate::ActionRequest;
use crate::QueryResource;
use fundamental_core::AnyError;
use fundamental_core::action::{Action as ActionMeta, ActionPath, ClientAction, QueryOperation};
#[cfg(feature = "ssr")]
use fundamental_server::ActionDispatcher;
use leptos::prelude::{LocalResource, Resource};
use serde::de::DeserializeOwned;

/// Converts an existing query resource or generated query hook into a resource handle.
#[doc(hidden)]
pub trait QuerySource {
    /// The successful query value.
    type Value: Clone + Send + Sync + 'static;
    /// The underlying Leptos resource type.
    type Resource;

    /// Builds the Leptos resource inside the component's reactive owner.
    fn into_resource(self) -> QueryResource<Self::Resource, Self::Value>;
}

impl<R, T> QuerySource for QueryResource<R, T>
where
    R: Clone + IntoFuture<Output = Result<T, AnyError>> + 'static,
    <R as IntoFuture>::IntoFuture: 'static,
    T: Clone + Send + Sync + 'static,
{
    type Value = T;
    type Resource = R;

    fn into_resource(self) -> QueryResource<Self::Resource, Self::Value> {
        self
    }
}

impl<M, T, P> QuerySource for ActionHook<M, QueryOperation, NoQuerySource, P>
where
    M: ClientAction<Operation = QueryOperation, Success = T> + ActionMeta<Input = ()> + 'static,
    T: Clone + Send + Sync + serde::Serialize + DeserializeOwned + 'static,
    P: HookPath<M> + Send + Sync,
{
    type Value = T;
    type Resource = Resource<Result<T, AnyError>>;

    fn into_resource(self) -> QueryResource<Self::Resource, Self::Value> {
        let path = self.path;
        let source = move || path.current();
        #[cfg(feature = "ssr")]
        {
            let dispatcher = leptos::prelude::use_context::<ActionDispatcher>();
            let fetcher = move |path| call_server_action::<M, T>(dispatcher.clone(), path, ());
            crate::use_query_prefetch!(source, fetcher)
        }
        #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
        {
            let fetcher =
                |path| <M as ClientAction>::call_at::<crate::WebActionTransport>(path, ());
            crate::use_query_prefetch!(source, fetcher)
        }
    }
}

impl<M, I, T, F, P> QuerySource for ActionHook<M, QueryOperation, ActionQuerySource<F>, P>
where
    M: ClientAction<Operation = QueryOperation, Success = T> + ActionMeta<Input = I> + 'static,
    I: Clone + PartialEq + Send + Sync + serde::Serialize + 'static,
    T: Clone + Send + Sync + serde::Serialize + DeserializeOwned + 'static,
    F: Fn() -> I + Send + Sync + 'static,
    P: HookPath<M> + Send + Sync,
{
    type Value = T;
    type Resource = Resource<Result<T, AnyError>>;

    fn into_resource(self) -> QueryResource<Self::Resource, Self::Value> {
        let ActionQuerySource(input) = self.source;
        let path = self.path;
        let source = move || (path.current(), input());
        #[cfg(feature = "ssr")]
        {
            let dispatcher = leptos::prelude::use_context::<ActionDispatcher>();
            let fetcher = move |(path, input): (ActionPath<M>, I)| {
                call_server_action::<M, T>(dispatcher.clone(), path, input)
            };
            crate::use_query_prefetch!(source, fetcher)
        }
        #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
        {
            let fetcher = |(path, input): (ActionPath<M>, I)| {
                <M as ClientAction>::call_at::<crate::WebActionTransport>(path, input)
            };
            crate::use_query_prefetch!(source, fetcher)
        }
    }
}

impl<M, T, P> QuerySource for ActionHook<M, QueryOperation, BlockingSource<NoQuerySource>, P>
where
    M: ClientAction<Operation = QueryOperation, Success = T> + ActionMeta<Input = ()> + 'static,
    T: Clone + Send + Sync + serde::Serialize + DeserializeOwned + 'static,
    P: HookPath<M> + Send + Sync,
{
    type Value = T;
    type Resource = Resource<Result<T, AnyError>>;

    fn into_resource(self) -> QueryResource<Self::Resource, Self::Value> {
        let path = self.path;
        let source = move || path.current();
        #[cfg(feature = "ssr")]
        {
            let dispatcher = leptos::prelude::use_context::<ActionDispatcher>();
            let fetcher = move |path| call_server_action::<M, T>(dispatcher.clone(), path, ());
            crate::use_query_prefetch!(@blocking source, fetcher)
        }
        #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
        {
            let fetcher =
                |path| <M as ClientAction>::call_at::<crate::WebActionTransport>(path, ());
            crate::use_query_prefetch!(@blocking source, fetcher)
        }
    }
}

impl<M, I, T, F, P> QuerySource
    for ActionHook<M, QueryOperation, BlockingSource<ActionQuerySource<F>>, P>
where
    M: ClientAction<Operation = QueryOperation, Success = T> + ActionMeta<Input = I> + 'static,
    I: Clone + PartialEq + Send + Sync + serde::Serialize + 'static,
    T: Clone + Send + Sync + serde::Serialize + DeserializeOwned + 'static,
    F: Fn() -> I + Send + Sync + 'static,
    P: HookPath<M> + Send + Sync,
{
    type Value = T;
    type Resource = Resource<Result<T, AnyError>>;

    fn into_resource(self) -> QueryResource<Self::Resource, Self::Value> {
        let ActionQuerySource(input) = self.source.0;
        let path = self.path;
        let source = move || (path.current(), input());
        #[cfg(feature = "ssr")]
        {
            let dispatcher = leptos::prelude::use_context::<ActionDispatcher>();
            let fetcher = move |(path, input): (ActionPath<M>, I)| {
                call_server_action::<M, T>(dispatcher.clone(), path, input)
            };
            crate::use_query_prefetch!(@blocking source, fetcher)
        }
        #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
        {
            let fetcher = |(path, input): (ActionPath<M>, I)| {
                <M as ClientAction>::call_at::<crate::WebActionTransport>(path, input)
            };
            crate::use_query_prefetch!(@blocking source, fetcher)
        }
    }
}

impl<M, T, P> QuerySource for ActionHook<M, QueryOperation, LocalSource<NoQuerySource>, P>
where
    M: ClientAction<Operation = QueryOperation, Success = T> + ActionMeta<Input = ()> + 'static,
    T: Clone + Send + Sync + DeserializeOwned + 'static,
    P: HookPath<M>,
{
    type Value = T;
    type Resource = LocalResource<Result<T, AnyError>>;

    fn into_resource(self) -> QueryResource<Self::Resource, Self::Value> {
        let path = self.path;
        let fetcher =
            move || <M as ClientAction>::call_at::<crate::WebActionTransport>(path.current(), ());
        crate::use_query!(fetcher)
    }
}

impl<M, I, T, F, P> QuerySource
    for ActionHook<M, QueryOperation, LocalSource<ActionQuerySource<F>>, P>
where
    M: ClientAction<Operation = QueryOperation, Success = T> + ActionMeta<Input = I> + 'static,
    I: Clone + PartialEq + serde::Serialize + 'static,
    T: Clone + Send + Sync + DeserializeOwned + 'static,
    F: Fn() -> I + 'static,
    P: HookPath<M>,
{
    type Value = T;
    type Resource = LocalResource<Result<T, AnyError>>;

    fn into_resource(self) -> QueryResource<Self::Resource, Self::Value> {
        let ActionQuerySource(input) = self.source.0;
        let path = self.path;
        let source = move || (path.current(), input());
        let fetcher = |(path, input): (ActionPath<M>, I)| {
            <M as ClientAction>::call_at::<crate::WebActionTransport>(path, input)
        };
        crate::use_query!(source, fetcher)
    }
}

/// Runs a query action in process during server rendering, at `path`.
#[cfg(feature = "ssr")]
async fn call_server_action<M, T>(
    dispatcher: Option<ActionDispatcher>,
    path: ActionPath<M>,
    input: M::Input,
) -> Result<T, AnyError>
where
    M: ClientAction<Operation = QueryOperation, Success = T> + ActionMeta + 'static,
    M::Input: serde::Serialize + 'static,
    T: DeserializeOwned + 'static,
{
    let dispatcher = dispatcher.ok_or_else(|| {
        crate::WebClientError::Transport(
            "the page request has no mounted action dispatcher".to_owned(),
        )
    })?;
    let request = ActionRequest::new(M::METHOD, &path, &input)?;
    let body = request.body().unwrap_or_default().as_bytes().to_vec();
    let body = dispatcher.dispatch_at::<M>(&request.target(), body).await?;
    serde_json::from_slice(&body)
        .map_err(|error| {
            crate::WebClientError::Transport(format!("deserialize action response: {error}"))
        })
        .map_err(Into::into)
}
