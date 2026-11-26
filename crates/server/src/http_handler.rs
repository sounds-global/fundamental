use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

use fundamental_core::AnyError;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::body::{decode_form, decode_json, decode_query, encode_body};
use crate::http_context::HttpContext;

/// A boxed future resolving to a serialized response body, borrowing the handler.
pub(crate) type HandlerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<u8>, AnyError>> + Send + 'a>>;

/// Where a handler's typed input is decoded from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputSource {
    /// Decode the input from the JSON request body.
    Body,
    /// Decode the input from the URL query string.
    Query,
    /// Decode the input from a form-urlencoded request body.
    Form,
    /// Decode from whichever body shape the request's `Content-Type` declares.
    BodyOrForm,
}

/// The media type a browser sends when it posts a `<form>` with no `enctype`.
const FORM_MEDIA_TYPE: &str = "application/x-www-form-urlencoded";
const MULTIPART_MEDIA_TYPE: &str = "multipart/form-data";

/// Decodes a handler's typed input `I` from `source`; an empty or undeclared body is JSON.
pub(crate) fn decode_input<I: DeserializeOwned>(
    source: InputSource,
    query: &str,
    body: &[u8],
    content_type: Option<&str>,
) -> Result<I, AnyError> {
    match source {
        InputSource::Body if body.is_empty() => decode_json(b"null"),
        InputSource::Body => decode_json(body),
        InputSource::Query => decode_query(query),
        InputSource::Form => decode_form(body),
        InputSource::BodyOrForm if is_form(content_type) => decode_form(body),
        InputSource::BodyOrForm if body.is_empty() => decode_json(b"null"),
        InputSource::BodyOrForm => decode_json(body),
    }
}

/// Whether a `Content-Type` names the form media type, case-insensitively and ignoring parameters.
pub(crate) fn is_form(content_type: Option<&str>) -> bool {
    media_type_is(content_type, FORM_MEDIA_TYPE)
}

/// Whether `content_type` is a `multipart/form-data` post.
pub(crate) fn is_multipart(content_type: Option<&str>) -> bool {
    media_type_is(content_type, MULTIPART_MEDIA_TYPE)
}

fn media_type_is(content_type: Option<&str>, media_type: &str) -> bool {
    content_type.is_some_and(|value| {
        value
            .split(';')
            .next()
            .unwrap_or(value)
            .trim()
            .eq_ignore_ascii_case(media_type)
    })
}

/// An object-safe handler: a context and a raw request body in, a raw response body out.
pub(crate) trait ErasedHandler<S>: Send + Sync {
    fn call<'a>(&'a self, context: HttpContext<S>, body: Vec<u8>) -> HandlerFuture<'a>;

    /// The content type the handler's body carries; `application/json` by default.
    fn content_type(&self) -> &'static str {
        "application/json"
    }
}

/// Adapts a typed handler into an [`ErasedHandler`], deserializing `I` and serializing `O`.
pub(crate) struct TypedHandler<S, I, O, H> {
    handler: H,
    source: InputSource,
    _types: PhantomData<fn(S, I) -> O>,
}

impl<S, I, O, H> TypedHandler<S, I, O, H> {
    /// A handler whose input is decoded from the JSON request body.
    pub(crate) fn new(handler: H) -> Self {
        Self::with_source(InputSource::Body, handler)
    }

    /// A handler whose input is decoded from `source`.
    pub(crate) fn with_source(source: InputSource, handler: H) -> Self {
        Self {
            handler,
            source,
            _types: PhantomData,
        }
    }
}

impl<S, I, O, H, Fut> ErasedHandler<S> for TypedHandler<S, I, O, H>
where
    S: Send + 'static,
    I: DeserializeOwned + Send + 'static,
    O: Serialize + Send + 'static,
    H: Fn(HttpContext<S>, I) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<O, AnyError>> + Send + 'static,
{
    fn call<'a>(&'a self, context: HttpContext<S>, body: Vec<u8>) -> HandlerFuture<'a> {
        let parsed = decode_input::<I>(
            self.source,
            context.query_raw(),
            &body,
            context.header("content-type"),
        );

        match parsed {
            Ok(input) => {
                let fut = (self.handler)(context, input);
                Box::pin(async move {
                    let output = fut.await?;
                    encode_body(&output)
                })
            }
            Err(err) => Box::pin(async move { Err(err) }),
        }
    }
}
