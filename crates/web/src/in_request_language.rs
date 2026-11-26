use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::sync::Arc;

use fundamental_core::i18n::{self, Translator};
use fundamental_server::HttpContext;
use leptos::prelude::{Owner, provide_context};

use crate::in_language::translator_for;

/// Runs `load` inside the language and address `ctx` negotiated, entered per poll.
///
/// The language scope is thread-local, so it is never held across an `.await`.
pub fn in_request_language<S, F>(ctx: &HttpContext<S>, load: F) -> InRequestLanguage<F>
where
    F: Future,
{
    let owner = ctx.address().cloned().map(|address| {
        let owner = Owner::new();
        owner.with(|| provide_context(address));
        owner
    });
    InRequestLanguage {
        translator: translator_for(ctx.locale()),
        owner,
        load: Box::pin(load),
    }
}

/// The future [`in_request_language`] returns.
#[must_use = "a load does nothing until it is awaited"]
pub struct InRequestLanguage<F> {
    translator: Option<Arc<dyn Translator>>,
    owner: Option<Owner>,
    load: Pin<Box<F>>,
}

impl<F: Future> Future for InRequestLanguage<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let this = self.get_mut();
        let load = &mut this.load;
        let translator = &this.translator;
        let mut poll = || match translator {
            Some(translator) => i18n::with(Arc::clone(translator), || load.as_mut().poll(cx)),
            None => load.as_mut().poll(cx),
        };
        match &this.owner {
            Some(owner) => owner.with(poll),
            None => poll(),
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::sync::Arc;

    use fundamental_core::address::{
        AddressSpace, LocalePrefixSet, PagePath, PageSlug, RequestAddress, SlugTable,
    };
    use fundamental_server::HttpContext;
    use leptos::prelude::use_context;

    use super::in_request_language;

    fn dutch_address() -> RequestAddress {
        let slugs = SlugTable::builder()
            .page("/why", &[PageSlug::current("nl", "waarom")])
            .build()
            .expect("well formed");
        let prefixes = LocalePrefixSet::new()
            .with("nl", "nl-NL".parse().expect("tag"))
            .expect("nl");
        let space = Arc::new(AddressSpace::new(prefixes, slugs));
        RequestAddress::new(space, "nl-NL".parse().expect("tag"), "/nl", "/")
    }

    #[tokio::test]
    async fn a_load_spells_a_page_at_the_requests_address() {
        let ctx = HttpContext::new((), "req").with_address(Some(dutch_address()));
        let spelled = in_request_language(&ctx, async {
            let before = use_context::<RequestAddress>()
                .map(|address| address.href_to(&PagePath::of_route("/why")));
            tokio::task::yield_now().await;
            let after = use_context::<RequestAddress>()
                .map(|address| address.href_to(&PagePath::of_route("/why")));
            (before, after)
        })
        .await;
        assert_eq!(
            spelled,
            (Some("/nl/waarom".to_owned()), Some("/nl/waarom".to_owned()))
        );
    }

    #[tokio::test]
    async fn a_load_without_an_address_space_finds_none() {
        let ctx = HttpContext::new((), "req");
        let found = in_request_language(&ctx, async { use_context::<RequestAddress>() }).await;
        assert!(found.is_none());
    }
}
