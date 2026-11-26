use crate::Peer;
use std::sync::Arc;

use fundamental_auth::AuthenticatedPrincipal;
use fundamental_core::address::{AddressSpace, RequestAddress, ResolvedAddress};
use fundamental_core::error::ShownError;
use fundamental_core::i18n::Locale;

use crate::{
    InboundRequestId, InboundTraceContext, Method, REQUEST_ID_HEADER, RequestBodyStream,
    RequestHeaders, RequestIdResolution,
};

/// A backend-neutral request: everything the router needs to dispatch.
pub struct ServerRequest<S> {
    method: Method,
    path: String,
    query: String,
    body: Vec<u8>,
    /// The body read as it arrives, for a streamed route; when set, `body` is empty.
    body_stream: Option<RequestBodyStream>,
    state: S,
    /// Decided by the backend at its outermost edge, or by the router; never a caller's value.
    request_id: Option<RequestIdResolution>,
    /// What the caller's `traceparent` may do on the router's span; `None` uses none of it.
    inbound_trace_context: Option<InboundTraceContext>,
    principal: Option<AuthenticatedPrincipal>,
    locale: Option<Locale>,
    /// Resolved display preferences, as `(name, value)` — see `PreferencePolicy`.
    preferences: Vec<(&'static str, &'static str)>,
    address: Option<RequestAddress>,
    headers: RequestHeaders,
    /// The socket the request arrived on; `None` for a caller that never crossed a network.
    peer: Option<Peer>,
    /// The authority the request was addressed to, never from a forwarding header; it only selects among configured origins.
    authority: Option<String>,
    /// The error this request's page answers, set only on the router's error-page request.
    shown_error: Option<ShownError>,
}

/// The parts of a [`ServerRequest`] dispatch consumes.
pub(crate) struct RequestParts<S> {
    pub(crate) body: Vec<u8>,
    pub(crate) body_stream: Option<RequestBodyStream>,
    pub(crate) state: S,
    pub(crate) request_id: String,
    pub(crate) principal: Option<AuthenticatedPrincipal>,
    pub(crate) locale: Option<Locale>,
    pub(crate) preferences: Vec<(&'static str, &'static str)>,
    pub(crate) address: Option<RequestAddress>,
    pub(crate) peer: Option<Peer>,
    pub(crate) query: String,
    pub(crate) headers: RequestHeaders,
    pub(crate) shown_error: Option<ShownError>,
}

impl<S> ServerRequest<S> {
    /// Creates a request for `method` and `path` over `state`, with an empty body and no principal.
    pub fn new(method: Method, path: impl Into<String>, state: S) -> Self {
        Self {
            method,
            path: path.into(),
            query: String::new(),
            body: Vec::new(),
            body_stream: None,
            state,
            request_id: None,
            inbound_trace_context: None,
            principal: None,
            locale: None,
            preferences: Vec::new(),
            address: None,
            headers: RequestHeaders::new(),
            peer: None,
            authority: None,
            shown_error: None,
        }
    }

    /// Sets the request body bytes.
    #[must_use]
    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    /// Sets the body as a stream read as it arrives, for a [streamed route](crate::Route::streamed).
    #[must_use]
    pub fn with_body_stream(mut self, body: RequestBodyStream) -> Self {
        self.body = Vec::new();
        self.body_stream = Some(body);
        self
    }

    /// Sets the raw URL query string (everything after `?`, without the `?`).
    #[must_use]
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.query = query.into();
        self
    }

    /// The raw URL query string, without the `?`; empty when there is none.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Sets the id the request is recorded under, as the backend decided it.
    #[must_use]
    pub fn with_request_id(mut self, resolution: RequestIdResolution) -> Self {
        self.request_id = Some(resolution);
        self
    }

    /// Sets what the caller's `traceparent` may do on the router's span, for a backend whose outermost span is the router's.
    #[must_use]
    pub fn with_inbound_trace_context(mut self, policy: InboundTraceContext) -> Self {
        self.inbound_trace_context = Some(policy);
        self
    }

    /// Attaches a resolved authenticated principal.
    #[must_use]
    pub fn with_principal(mut self, principal: Option<AuthenticatedPrincipal>) -> Self {
        self.principal = principal;
        self
    }

    /// Attaches the language this request is answered in.
    #[must_use]
    pub fn with_locale(mut self, locale: Option<Locale>) -> Self {
        self.locale = locale;
        self
    }

    /// Attaches the display preferences this request pinned.
    #[must_use]
    pub fn with_preferences(mut self, preferences: Vec<(&'static str, &'static str)>) -> Self {
        self.preferences = preferences;
        self
    }

    /// Attaches the request headers captured at the boundary.
    #[must_use]
    pub fn with_headers(mut self, headers: RequestHeaders) -> Self {
        self.headers = headers;
        self
    }

    /// Records the authority (`Host`, or HTTP/2 `:authority`) the request was addressed to.
    #[must_use]
    pub fn with_authority(mut self, authority: Option<String>) -> Self {
        self.authority = authority;
        self
    }

    /// The authority the request was addressed to, when the backend recorded one.
    pub(crate) fn authority(&self) -> Option<&str> {
        self.authority.as_deref()
    }

    /// Records the socket this request arrived on; set only from the accepted connection.
    #[must_use]
    pub fn with_peer(mut self, peer: impl Into<Peer>) -> Self {
        self.peer = Some(peer.into());
        self
    }

    /// The socket this request arrived on, if any.
    #[must_use]
    pub fn peer(&self) -> Option<Peer> {
        self.peer
    }

    /// The language negotiated at the boundary, before any address resolved one.
    pub(crate) fn locale(&self) -> Option<&Locale> {
        self.locale.as_ref()
    }

    /// Applies a resolved address: its locale, which outranks a negotiated one, and the page's address.
    pub(crate) fn addressed(
        mut self,
        resolved: &ResolvedAddress,
        space: Arc<AddressSpace>,
        served: bool,
    ) -> Self {
        if let Some(locale) = resolved.locale() {
            self.locale = Some(locale.clone());
            let address = RequestAddress::new(
                space,
                locale.clone(),
                resolved.prefix(),
                resolved.canonical_path(),
            );
            self.address = Some(match served {
                true => address,
                false => address.unserved(),
            });
        }
        self
    }

    /// The request an error page renders with: a `GET` of the same path, language, preferences, origin and id, nothing else.
    pub(crate) fn for_error_page(&self) -> Self
    where
        S: Clone,
    {
        Self {
            method: Method::Get,
            path: self.path.clone(),
            query: String::new(),
            body: Vec::new(),
            body_stream: None,
            state: self.state.clone(),
            request_id: self.request_id,
            inbound_trace_context: None,
            principal: None,
            locale: self.locale.clone(),
            preferences: self.preferences.clone(),
            address: None,
            headers: RequestHeaders::new(),
            peer: None,
            authority: self.authority.clone(),
            shown_error: None,
        }
    }

    /// Names the error this request's page answers.
    pub(crate) fn with_shown_error(mut self, shown_error: ShownError) -> Self {
        self.shown_error = Some(shown_error);
        self
    }

    /// The request method.
    pub(crate) fn method(&self) -> Method {
        self.method
    }

    /// The request path.
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    /// The id the request is recorded under, decided here under [`InboundRequestId::Ignore`] if the backend did not.
    pub(crate) fn resolve_request_id(&mut self) -> RequestIdResolution {
        let headers = &self.headers;
        *self.request_id.get_or_insert_with(|| {
            RequestIdResolution::resolve(headers.get(REQUEST_ID_HEADER), InboundRequestId::Ignore)
        })
    }

    /// What the caller's `traceparent` may do on the router's span, if anything.
    pub(crate) fn inbound_trace_context(&self) -> Option<InboundTraceContext> {
        self.inbound_trace_context
    }

    /// The headers captured at the boundary.
    pub(crate) fn headers(&self) -> &RequestHeaders {
        &self.headers
    }

    /// Decomposes the request into the parts dispatch consumes.
    pub(crate) fn into_parts(self) -> RequestParts<S> {
        RequestParts {
            body: self.body,
            body_stream: self.body_stream,
            state: self.state,
            request_id: self
                .request_id
                .unwrap_or_else(RequestIdResolution::minted)
                .id()
                .to_string(),
            principal: self.principal,
            locale: self.locale,
            preferences: self.preferences,
            address: self.address,
            peer: self.peer,
            query: self.query,
            headers: self.headers,
            shown_error: self.shown_error,
        }
    }
}
