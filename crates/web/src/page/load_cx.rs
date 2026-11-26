use std::any::TypeId;
use std::sync::Arc;

use fundamental_auth::AuthenticatedPrincipal;
use fundamental_core::Error as _;
use fundamental_core::i18n::Locale;
use fundamental_core::validate::{Validate, ValidationError, ValidationErrors};
use fundamental_server::HttpContext;
use serde::de::DeserializeOwned;
use tracing::info;

use super::page_error::PageError;
use super::page_refusal::PageRefusal;
use super::public_params::PublicParams;
use super::public_segment::PublicSegment;

/// The request as a page loader sees it, with no access to the raw path parameters.
pub struct LoadCx<S> {
    ctx: HttpContext<S>,
    public: Arc<PublicParams>,
    locale: Locale,
}

impl<S: Clone> Clone for LoadCx<S> {
    fn clone(&self) -> Self {
        Self {
            ctx: self.ctx.clone(),
            public: Arc::clone(&self.public),
            locale: self.locale.clone(),
        }
    }
}

impl<S> LoadCx<S> {
    pub(crate) fn new(ctx: HttpContext<S>, public: Arc<PublicParams>) -> Self {
        let locale = ctx.locale().cloned().unwrap_or_else(undetermined);
        Self {
            ctx,
            public,
            locale,
        }
    }

    /// The application state: the stores and services a loader calls.
    #[must_use]
    pub fn state(&self) -> &S {
        self.ctx.state()
    }

    /// The value of the reader's cookie `name`; [`None`] when absent or sent more than once.
    #[must_use]
    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.ctx.cookie(name)
    }

    /// The signed-in reader, if there is one.
    #[must_use]
    pub fn principal(&self) -> Option<&AuthenticatedPrincipal> {
        self.ctx.principal_opt()
    }

    /// The signed-in reader.
    ///
    /// # Errors
    /// [`PageRefusal::SignIn`] when nobody is signed in, which shows the gate.
    pub fn signed_in(&self) -> Result<&AuthenticatedPrincipal, PageRefusal> {
        self.principal().ok_or(PageRefusal::SignIn)
    }

    /// The language the page renders in; `und` on a surface with no locale policy.
    #[must_use]
    pub fn locale(&self) -> &Locale {
        &self.locale
    }

    /// The request's correlation id.
    #[must_use]
    pub fn request_id(&self) -> &str {
        self.ctx.request_id()
    }

    /// The query, decoded into `Q` and validated, every field's failure collected.
    ///
    /// # Errors
    /// [`PageRefusal::Invalid`] when the query does not decode or does not validate — answered
    /// as a `400`.
    pub fn query<Q: DeserializeOwned + Validate>(&self) -> Result<Q, PageRefusal> {
        let query: Q = self.ctx.query().map_err(|error| {
            // The decoder's message can quote what the reader typed, so only its code is logged.
            info!(
                request_id = self.request_id(),
                code = error.error_code(),
                "page query did not decode"
            );
            PageRefusal::Invalid(ValidationErrors::single(ValidationError::invalid_format(
                "query",
                "does not decode",
            )))
        })?;
        query.validate().map_err(PageRefusal::Invalid)?;
        Ok(query)
    }

    /// The raw value of the route parameter `name`, which a policy declared public with
    /// [`public_param`](crate::PagePolicy::public_param).
    ///
    /// # Errors
    /// [`PageRefusal::Failed`] with `FW_PAGE_016` when no policy of the route declared it raw —
    /// a mistake in the page, answered as a `500` rather than a read of an unguarded id.
    pub fn public_param(&self, name: &str) -> Result<&str, PageRefusal> {
        if !self.public.raw.contains(&name) {
            return Err(undeclared(name));
        }
        self.ctx.params().get(name).ok_or_else(|| undeclared(name))
    }

    /// The route parameter `name` as the [`PublicSegment`] `T` a policy declared it as.
    ///
    /// # Errors
    /// [`PageRefusal::Unavailable`] when the word is not one of `T`'s, so an address naming no
    /// step is not found; [`PageRefusal::Failed`] with `FW_PAGE_016` when no policy declared
    /// `name` a segment of type `T`.
    pub fn public_segment<T: PublicSegment>(&self, name: &str) -> Result<T, PageRefusal> {
        let declared = self
            .public
            .segments
            .iter()
            .any(|(declared, ty)| *declared == name && *ty == TypeId::of::<T>());
        if !declared {
            return Err(undeclared(name));
        }
        let word = self
            .ctx
            .params()
            .get(name)
            .ok_or_else(|| undeclared(name))?;
        T::parse(word).ok_or(PageRefusal::Unavailable)
    }
}

fn undetermined() -> Locale {
    Locale::new("und", None::<&str>)
}

fn undeclared(name: &str) -> PageRefusal {
    PageRefusal::from(PageError::UndeclaredParam {
        param: name.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fundamental_core::i18n::Locale;
    use fundamental_server::HttpContext;

    use super::LoadCx;
    use crate::page::public_params::PublicParams;

    fn cx(locale: Option<Locale>) -> LoadCx<()> {
        LoadCx::new(
            HttpContext::new((), "req").with_locale(locale),
            Arc::new(PublicParams::default()),
        )
    }

    fn locale(tag: &str) -> Locale {
        tag.parse().expect("a language tag")
    }

    #[test]
    fn a_loader_reads_the_language_the_policy_negotiated() {
        assert_eq!(cx(Some(locale("nl"))).locale(), &locale("nl"));
    }

    #[test]
    fn a_loader_reads_one_of_the_reader_s_cookies_by_its_name() {
        let headers: fundamental_server::RequestHeaders = [(
            "cookie".to_owned(),
            "theme=dark; __Host-session=abc".to_owned(),
        )]
        .into_iter()
        .collect();
        let cx = LoadCx::new(
            HttpContext::new((), "req").with_headers(headers),
            Arc::new(PublicParams::default()),
        );
        assert_eq!(cx.cookie("__Host-session"), Some("abc"));
        assert_eq!(cx.cookie("theme"), Some("dark"));
        assert_eq!(cx.cookie("Theme"), None, "names match case-sensitively");
        assert_eq!(cx.cookie("absent"), None);
    }

    #[test]
    fn a_surface_that_negotiates_nothing_answers_in_no_language_of_its_own() {
        assert_eq!(cx(None).locale(), &locale("und"));
    }
}
