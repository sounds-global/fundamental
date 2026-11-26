use crate::{HttpContext, Method, RequestHeaders};

/// What kind of caller a request is: a person navigating or a program reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Audience {
    /// A top-level browser navigation. Wants a redirect.
    Navigation,
    /// Something reading a response programmatically. Wants a status.
    Programmatic,
}

impl Audience {
    /// Classify a request.
    ///
    /// `Sec-Fetch-Mode` decides where present, since script cannot forge it; otherwise only an
    /// `Accept` naming HTML makes a person, failing closed.
    #[must_use]
    pub fn of<S>(context: &HttpContext<S>) -> Self {
        Self::classify(context.method(), context.headers())
    }

    /// Classify a request from its method and headers alone, as [`of`](Self::of) does.
    #[must_use]
    pub fn classify(method: Option<Method>, headers: &RequestHeaders) -> Self {
        let navigable = matches!(method, Some(Method::Get));
        match headers.get("sec-fetch-mode") {
            Some("navigate") => Self::Navigation,
            Some(_) => Self::Programmatic,
            None if navigable && accepts_html(headers) => Self::Navigation,
            None => Self::Programmatic,
        }
    }
}

/// Whether the caller asked for HTML by name; `*/*` does not count.
fn accepts_html(headers: &RequestHeaders) -> bool {
    headers
        .get("accept")
        .is_some_and(|accept| accept.contains("text/html"))
}
