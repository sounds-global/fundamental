/// Creates a browser-only resource that tracks reactive source changes.
#[macro_export]
macro_rules! use_query {
    ($fetcher:expr $(,)?) => {{
        let state = ::leptos::prelude::RwSignal::new($crate::QueryState::Pending);
        let fetcher = $fetcher;
        let writes = $crate::Writes::current();
        let resource = ::leptos::prelude::LocalResource::new(move || {
            $crate::Writes::track(writes);
            $crate::track_query(state, fetcher())
        });
        let retry_resource = resource;
        $crate::QueryResource::new(
            resource,
            state,
            ::leptos::prelude::Callback::new(move |()| retry_resource.refetch()),
        )
    }};

    ($source:expr, $fetcher:expr $(,)?) => {{
        let source = $source;
        let fetcher = $fetcher;
        let state = ::leptos::prelude::RwSignal::new($crate::QueryState::Pending);
        let writes = $crate::Writes::current();
        let resource = ::leptos::prelude::LocalResource::new(move || {
            $crate::Writes::track(writes);
            let input = source();
            $crate::track_query(state, fetcher(input))
        });
        let retry_resource = resource;
        $crate::QueryResource::new(
            resource,
            state,
            ::leptos::prelude::Callback::new(move |()| retry_resource.refetch()),
        )
    }};
}
