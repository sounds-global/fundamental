use std::sync::Arc;

use fundamental_core::Error as _;
use fundamental_core::address::RequestAddress;
use fundamental_core::i18n::Locale;
use tracing::{debug, warn};

use crate::{
    Audience, FetchSite, HttpContext, LanguageCookie, LanguageVerdict, LocalePolicy, Method,
    Middleware, MiddlewareResult, Next, Redirect, ReturnPath, request_return_path,
};

/// The query parameter a language link carries, one word in every language.
const PARAM: &str = "lang";

/// Records the language a visitor chose, and puts them back where they were.
///
/// A layer rather than a route, so the return location is rebuilt from the matched template
/// rather than taken from the caller. The choice is recorded only when `Sec-Fetch-Site` is
/// absent, `same-origin`, `same-site` or `none`, so another site cannot set it. Mount it
/// before [`PageAuth`](crate::PageAuth).
#[derive(Clone, Debug)]
pub struct LocaleChoice {
    cookie: LanguageCookie,
    policy: Arc<LocalePolicy>,
    arrivals: bool,
}

impl LocaleChoice {
    /// A recorder for the languages `policy` negotiates, writing `cookie`.
    #[must_use]
    pub fn new(policy: &LocalePolicy, cookie: LanguageCookie) -> Self {
        Self {
            cookie,
            policy: Arc::new(policy.clone()),
            arrivals: false,
        }
    }

    /// The same recorder, also sending a reader arriving from elsewhere at a default-language page to their own language.
    ///
    /// A navigation not made from the site itself is answered `302`; every such answer varies
    /// on the headers that decide it, and a redirect is never stored.
    #[must_use]
    pub fn with_arrivals(mut self) -> Self {
        self.arrivals = true;
        self
    }

    /// A recorder built entirely from `policy`; `None` when the policy reads no cookie.
    #[must_use]
    pub fn of(policy: &LocalePolicy, development: bool) -> Option<Self> {
        LanguageCookie::of(policy)
            .map(|cookie| Self::new(policy, cookie.for_development(development)))
    }

    /// Decide one request; a pure function of the context, matched exactly against an offered locale.
    #[must_use]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn verdict<S>(&self, context: &HttpContext<S>) -> LanguageVerdict {
        if !matches!(context.method(), Some(Method::Get)) {
            return LanguageVerdict::Untouched;
        }
        let Some(chosen) = chosen(context.query_raw()) else {
            return LanguageVerdict::Untouched;
        };
        if Audience::of(context) != Audience::Navigation {
            return LanguageVerdict::Ignored;
        }
        // Parsed before it is logged, which bounds a caller's value to `language[-region]`.
        let Ok(language) = chosen.parse::<Locale>() else {
            debug!(
                tag = "malformed",
                outcome = "ignored",
                "language choice not recorded"
            );
            return LanguageVerdict::Untouched;
        };
        if !self.policy.supported().contains(&language) {
            debug!(
                tag = %language,
                outcome = "not offered",
                "language choice not recorded"
            );
            return LanguageVerdict::Untouched;
        }
        let Some(return_to) = request_return_path(context) else {
            debug!(
                tag = %language,
                outcome = "no return path",
                "language choice not recorded"
            );
            return LanguageVerdict::Untouched;
        };
        let return_to = return_to.without_query_param(PARAM);
        let site = FetchSite::parse(context.header("sec-fetch-site"));
        match site {
            FetchSite::SameOrigin | FetchSite::SameSite | FetchSite::None | FetchSite::Absent => {
                LanguageVerdict::Record {
                    language,
                    return_to,
                }
            }
            _ => {
                debug!(
                    tag = %language,
                    sec_fetch_site = site.as_str(),
                    outcome = "declined",
                    "language choice not recorded"
                );
                LanguageVerdict::Declined { return_to }
            }
        }
    }

    /// The cookie a recorded choice is written with.
    #[must_use]
    pub fn cookie(&self) -> &LanguageCookie {
        &self.cookie
    }

    /// The languages a link may name — the policy's own supported set.
    #[must_use]
    pub fn offered(&self) -> &[Locale] {
        self.policy.supported()
    }

    /// Answers a reader arriving at a default-language address in their own language; `true` when redirected.
    fn arrive<S>(&self, ctx: &HttpContext<S>) -> bool {
        if !self.arrivals {
            return false;
        }
        let Some(address) = default_language_page(ctx) else {
            return false;
        };
        // A navigation to this address could be sent elsewhere, so the answer names every header that decides it.
        let mut vary = self.policy.vary().to_vec();
        vary.extend(["Sec-Fetch-Site", "Sec-Fetch-Mode", "Accept"]);
        ctx.set_response_header("vary", vary.join(", "));
        if Audience::of(ctx) != Audience::Navigation
            || FetchSite::parse(ctx.header("sec-fetch-site")) == FetchSite::SameOrigin
        {
            return false;
        }
        let Some(location) = self.elsewhere(ctx, address) else {
            return false;
        };
        match Redirect::found(location) {
            Ok(redirect) => {
                answer(ctx, redirect);
                true
            }
            Err(error) => {
                warn!(
                    route = %ctx.route_template().unwrap_or("<unmatched>"),
                    error = error.error_code(),
                    outcome = "not redirected",
                    "language arrival"
                );
                false
            }
        }
    }

    /// The page at `address` in the language the policy negotiates, or `None` when that is its own.
    fn elsewhere<S>(&self, ctx: &HttpContext<S>, address: &RequestAddress) -> Option<String> {
        let wanted = self.policy.negotiate(
            ctx.header("accept-language"),
            ctx.headers().get_all("cookie"),
        );
        if wanted == *address.locale() {
            return None;
        }
        let tag = wanted.to_string();
        let (_, page) = address
            .alternates()
            .into_iter()
            .find(|(hreflang, _)| *hreflang == tag)?;
        debug!(
            route = %ctx.route_template().unwrap_or("<unmatched>"),
            language = %wanted,
            outcome = "sent to their language",
            "language arrival"
        );
        Some(match ctx.query_raw() {
            "" => page,
            query => format!("{page}?{query}"),
        })
    }
}

/// The address of the page a `GET` asked for, when served unprefixed in the default language.
fn default_language_page<S>(ctx: &HttpContext<S>) -> Option<&RequestAddress> {
    let address = ctx.address()?;
    let at_default = matches!(ctx.method(), Some(Method::Get))
        && address.is_served()
        && address.prefix().trim_start_matches('/').is_empty()
        && address.space().default_language() == Some(address.locale());
    at_default.then_some(address)
}

/// The value of the single `lang` parameter; `None` for none or several.
fn chosen(query: &str) -> Option<&str> {
    let mut seen = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if key != PARAM {
            continue;
        }
        if seen.is_some() {
            return None;
        }
        seen = Some(value);
    }
    seen
}

/// Records a language choice on any route of the router it is mounted on.
impl<S> Middleware<S> for LocaleChoice
where
    S: Send + Sync + 'static,
{
    async fn handle(
        &self,
        ctx: HttpContext<S>,
        body: Vec<u8>,
        next: Next<'_, S>,
    ) -> MiddlewareResult {
        match self.verdict(&ctx) {
            LanguageVerdict::Untouched => {
                if self.arrive(&ctx) {
                    return Ok(Vec::new());
                }
                next.run(ctx, body).await
            }
            LanguageVerdict::Ignored => {
                // A navigation gets a `303` and a programmatic caller the page, so a cache must key on the mode.
                ctx.set_response_header("vary", "sec-fetch-mode");
                next.run(ctx, body).await
            }
            LanguageVerdict::Record {
                language,
                return_to,
            } => {
                let Some(redirect) = see_other(&ctx, &located(&ctx, Some(&language), &return_to))
                else {
                    return next.run(ctx, body).await;
                };
                // The route template, never the rebuilt path, which can carry a personal identifier.
                debug!(
                    route = %ctx.route_template().unwrap_or("<unmatched>"),
                    language = %language,
                    sec_fetch_site = FetchSite::parse(ctx.header("sec-fetch-site")).as_str(),
                    outcome = "recorded",
                    "language choice"
                );
                ctx.set_response_header("set-cookie", self.cookie.set(&language));
                answer(&ctx, redirect);
                Ok(Vec::new())
            }
            LanguageVerdict::Declined { return_to } => {
                match see_other(&ctx, &located(&ctx, None, &return_to)) {
                    Some(redirect) => {
                        answer(&ctx, redirect);
                        Ok(Vec::new())
                    }
                    None => next.run(ctx, body).await,
                }
            }
        }
    }
}

/// Where `return_to` is read in `language`: the page at its address in that language, where it has one.
fn located<S>(ctx: &HttpContext<S>, language: Option<&Locale>, return_to: &ReturnPath) -> String {
    let location = return_to.as_location();
    let Some(address) = ctx.address() else {
        return location.to_owned();
    };
    let space = address.space();
    let language = language.unwrap_or_else(|| address.locale());
    let origin_of = |locale: &Locale| {
        space
            .language_origins()
            .iter()
            .find(|(served, _)| served == locale)
            .map(|(_, origin)| origin)
    };
    if origin_of(language) != origin_of(address.locale())
        && let Some(url) = space.url_in(language, location)
    {
        return url.to_string();
    }
    space.address_in(language, location)
}

/// The `303` to `location`, or `None`, logged, when the location cannot survive the wire.
fn see_other<S>(ctx: &HttpContext<S>, location: &str) -> Option<Redirect> {
    match Redirect::see_other(location) {
        Ok(redirect) => Some(redirect),
        Err(error) => {
            warn!(
                route = %ctx.route_template().unwrap_or("<unmatched>"),
                error = error.error_code(),
                outcome = "not redirected",
                "language choice"
            );
            None
        }
    }
}

/// Answers with `redirect`, never cached: the `Set-Cookie` is the payload.
fn answer<S>(ctx: &HttpContext<S>, redirect: Redirect) {
    ctx.redirect(redirect);
    ctx.set_response_header("cache-control", "no-store");
}

#[cfg(test)]
mod tests {
    use super::chosen;

    #[test]
    fn the_parameter_is_found_wherever_it_sits() {
        assert_eq!(chosen("lang=nl"), Some("nl"));
        assert_eq!(chosen("a=1&lang=nl&b=2"), Some("nl"));
        assert_eq!(chosen("a=1&lang=nl"), Some("nl"));
    }

    #[test]
    fn a_key_that_merely_starts_with_it_is_not_it() {
        // A prefix match would make `?language=nl` trigger a redirect.
        assert_eq!(chosen("language=nl"), None);
        assert_eq!(chosen("langue=nl"), None);
    }

    #[test]
    fn two_choices_at_once_are_no_choice() {
        assert_eq!(chosen("lang=nl&lang=en"), None);
        assert_eq!(chosen("lang=nl&a=1&lang=nl"), None);
    }

    #[test]
    fn an_empty_or_absent_parameter_reads_as_written() {
        assert_eq!(chosen(""), None);
        assert_eq!(chosen("a=1&b=2"), None);
        assert_eq!(chosen("lang="), Some(""));
        assert_eq!(chosen("lang"), Some(""));
    }
}
