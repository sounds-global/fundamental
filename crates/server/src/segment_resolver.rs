use std::future::Future;
use std::pin::Pin;

use fundamental_core::AnyError;
use fundamental_core::i18n::Locale;

use crate::{ResolvedSegment, SegmentResolution};

/// Resolves one path parameter declared with [`Router::segment`](crate::Router::segment) to the record's id.
pub trait SegmentResolver: Send + Sync + 'static {
    /// The path parameter this resolves, as the route templates name it (`team`).
    fn param(&self) -> &str;

    /// The parameter whose record scopes this one, resolved first and handed to [`resolve`](Self::resolve).
    fn scope(&self) -> Option<&str> {
        None
    }

    /// The authorization namespace whose grant on the resolved id affirms the record; `None` leaves it to the loader.
    fn namespace(&self) -> Option<&str> {
        None
    }

    /// A fresh id that names no record, in the format the route reads.
    fn void_id(&self) -> String;

    /// What `raw` names, within `scope` when one is declared.
    ///
    /// # Errors
    ///
    /// When the record could not be looked up, so an outage is not mistaken for a missing record.
    fn resolve<'a>(
        &'a self,
        raw: &'a str,
        scope: Option<&'a ResolvedSegment>,
        locale: Option<&'a Locale>,
    ) -> impl Future<Output = Result<SegmentResolution, AnyError>> + Send + 'a;
}

/// A resolution in flight, borrowing its resolver.
pub(crate) type ResolutionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<SegmentResolution, AnyError>> + Send + 'a>>;

/// [`SegmentResolver`] made object-safe, as the router stores it.
pub(crate) trait ErasedSegmentResolver: Send + Sync {
    fn param(&self) -> &str;
    fn scope(&self) -> Option<&str>;
    fn namespace(&self) -> Option<&str>;
    fn void_id(&self) -> String;
    fn resolve<'a>(
        &'a self,
        raw: &'a str,
        scope: Option<&'a ResolvedSegment>,
        locale: Option<&'a Locale>,
    ) -> ResolutionFuture<'a>;
}

impl<R: SegmentResolver> ErasedSegmentResolver for R {
    fn param(&self) -> &str {
        SegmentResolver::param(self)
    }

    fn scope(&self) -> Option<&str> {
        SegmentResolver::scope(self)
    }

    fn namespace(&self) -> Option<&str> {
        SegmentResolver::namespace(self)
    }

    fn void_id(&self) -> String {
        SegmentResolver::void_id(self)
    }

    fn resolve<'a>(
        &'a self,
        raw: &'a str,
        scope: Option<&'a ResolvedSegment>,
        locale: Option<&'a Locale>,
    ) -> ResolutionFuture<'a> {
        Box::pin(SegmentResolver::resolve(self, raw, scope, locale))
    }
}
