/// Creates a server-capable resource whose value is serialized for hydration.
///
/// `@blocking` builds it with `Resource::new_blocking`, holding the page until it resolves.
#[macro_export]
macro_rules! use_query_prefetch {
    (@build $constructor:ident, $source:expr, $fetcher:expr) => {{
        let state = ::leptos::prelude::RwSignal::new($crate::QueryState::Pending);
        let source = $source;
        let fetcher = $fetcher;
        let writes = $crate::Writes::current();
        let resource = ::leptos::prelude::Resource::$constructor(
            move || (source(), $crate::Writes::seen(writes)),
            move |(source, _)| $crate::track_query(state, $crate::prefetch_future(fetcher(source))),
        );
        let retry_resource = resource;
        $crate::QueryResource::new(
            resource,
            state,
            ::leptos::prelude::Callback::new(move |()| retry_resource.refetch()),
        )
    }};

    (@blocking $source:expr, $fetcher:expr $(,)?) => {{
        let fetcher = $fetcher;
        let page = ::leptos::prelude::use_context::<$crate::PageContext>();
        $crate::use_query_prefetch!(@build new_blocking, $source, move |source| {
            $crate::hold_page(page.clone(), fetcher(source))
        })
    }};

    (@blocking $fetcher:expr $(,)?) => {{
        let fetcher = $fetcher;
        $crate::use_query_prefetch!(@blocking || (), move |()| fetcher())
    }};

    ($source:expr, $fetcher:expr $(,)?) => {
        $crate::use_query_prefetch!(@build new, $source, $fetcher)
    };

    ($fetcher:expr $(,)?) => {{
        let fetcher = $fetcher;
        $crate::use_query_prefetch!(@build new, || (), move |()| fetcher())
    }};
}
