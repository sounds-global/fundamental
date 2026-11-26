//! [`ServerError`], the error type for every fallible HTTP server operation.

use fundamental_macros::Error;

use crate::{FetchSite, ObservedOrigin, PathShape};

/// Result alias for fallible HTTP server operations.
pub type ServerResult<T> = Result<T, ServerError>;

/// An error from an HTTP server operation.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ServerError {
    /// Two routes were registered for the same method and path.
    #[error(
        code = "API_001",
        message = "Route already registered: {method} {path}",
        status = 500
    )]
    RouteConflict {
        /// The method whose path collided.
        method: String,
        /// The path that was registered twice.
        path: String,
    },

    /// No route matched the request path for any method.
    #[error(
        code = "API_002",
        message = "No route for {method} {path}",
        status = 404
    )]
    NotFound {
        /// The request method.
        method: String,
        /// The shape of the path that matched no route, never the path itself.
        path: PathShape,
    },

    /// The request used a method this server does not support; it carries no token.
    #[error(code = "API_003", message = "Unsupported method", status = 405)]
    UnsupportedMethod,

    /// The path exists, but not for the request's method.
    #[error(
        code = "API_004",
        message = "Method {method} not allowed for {path}",
        status = 405
    )]
    MethodNotAllowed {
        /// The request method.
        method: String,
        /// The shape of the path that exists for another method.
        path: PathShape,
    },

    /// A handler asked for a path parameter the route does not capture.
    #[error(
        code = "API_005",
        message = "Unknown path parameter: {name}",
        status = 500
    )]
    MissingPathParam {
        /// The parameter name the handler requested.
        name: String,
    },

    /// A path parameter could not be parsed into the requested type.
    #[error(
        code = "API_006",
        message = "Invalid path parameter {name}: {message}",
        status = 400
    )]
    InvalidPathParam {
        /// The parameter name.
        name: String,
        /// Why the value could not be parsed.
        message: String,
    },

    /// The request body did not match the handler's input type.
    #[error(code = "API_007", message = "Invalid request body: {0}", status = 400)]
    Deserialize(String),

    /// The handler's output could not be serialized into a response.
    #[error(
        code = "API_008",
        message = "Failed to serialize response: {0}",
        status = 500
    )]
    Serialize(String),

    /// The request was not authenticated; `#[derive(Error)]` keys `unauthenticated()` on its `401`.
    #[error(
        code = "API_009",
        message = "Authentication required: {0}",
        status = 401
    )]
    Unauthenticated(String),

    /// The server could not bind its configured address.
    #[error(
        code = "API_010",
        message = "Failed to bind {address}: {message}",
        status = 503,
        retryable
    )]
    Bind {
        /// The address the server tried to bind.
        address: String,
        /// The underlying bind failure.
        message: String,
    },

    /// An unexpected internal error.
    #[error(code = "API_011", message = "Internal error: {0}", status = 500)]
    Internal(String),

    /// A static asset directory could not be read while registering it.
    #[error(
        code = "API_012",
        message = "Failed to load static assets from {path}: {message}",
        status = 500
    )]
    AssetLoad {
        /// The directory whose load failed.
        path: String,
        /// The underlying filesystem error.
        message: String,
    },

    /// The server mounts routes that need a principal but has no verifier, so each would answer `401` forever.
    #[error(
        code = "API_013",
        message = "No principal verifier configured; these routes can never be satisfied: {routes}",
        status = 500
    )]
    MissingVerifier {
        /// The unsatisfiable routes, as `METHOD /path`, comma separated.
        routes: String,
    },

    /// The server mounts writes an identified caller can reach that authorize no object, and the app asked for them refused.
    #[error(
        code = "API_017",
        message = "These writes are reachable by any authenticated caller and do not authorize the object they change: {routes}",
        status = 500
    )]
    UnauthorizedWrites {
        /// The unguarded writes, as `METHOD /path`, comma separated.
        routes: String,
    },

    /// The server reads a credential from a cookie but has no verifier, so every caller would be silently anonymous.
    #[error(
        code = "API_014",
        message = "A credential source reads the {cookie} cookie, but no verifier was configured to verify it; attach one with `with_verifier`, or leave the source as `CredentialSource::BearerHeader`",
        status = 500
    )]
    CredentialSourceWithoutVerifier {
        /// The configured cookie name. Never a value.
        cookie: String,
    },

    /// An unsafe request authenticated by a cookie could not be shown to come from this origin.
    ///
    /// See [`guard_cookie_write`](crate::guard_cookie_write) for the signals accepted.
    #[error(
        code = "API_015",
        message = "Cross-site write refused: a cookie-authenticated write must come from this origin (Sec-Fetch-Site: {site}, Origin: {origin})",
        status = 403
    )]
    CrossSiteWrite {
        /// The `Sec-Fetch-Site` the request carried, as a class.
        site: FetchSite,
        /// The `Origin` the request carried, parsed or classed — never the header.
        origin: ObservedOrigin,
    },

    /// A [`Redirect`](crate::Redirect) was asked for with a location that cannot be a `Location` header.
    ///
    /// A `500`, since the application built the target; the reason never echoes the location.
    #[error(code = "API_016", message = "Cannot redirect: {reason}", status = 500)]
    InvalidRedirect {
        /// Which of the constructor's checks refused it. A fixed phrase.
        reason: &'static str,
    },

    /// A security header could not be built or sent; refused before the server binds, never skipped.
    #[error(
        code = "API_018",
        message = "The {header} header cannot be sent: {reason}",
        status = 500
    )]
    SecurityHeaderInvalid {
        /// The header, in lower case.
        header: &'static str,
        /// What is wrong with it. A fixed phrase.
        reason: &'static str,
    },

    /// A document was registered at a path something else already serves, or the reverse.
    #[error(
        code = "API_019",
        message = "Two things are served at {path}: a document and {held_by}",
        status = 500
    )]
    PathTaken {
        /// The contested path.
        path: String,
        /// What else claims it: an asset, a route, or another document.
        held_by: &'static str,
    },

    /// A request body ran past its route's bound; nothing past it was read.
    #[error(
        code = "API_020",
        message = "The request is larger than the {max} bytes this address accepts",
        status = 413
    )]
    PayloadTooLarge {
        /// The bound, in bytes.
        max: u64,
    },

    /// The request did not finish within its deadline.
    #[error(
        code = "API_021",
        message = "The request took too long and was stopped. Try again",
        status = 408,
        retryable
    )]
    RequestTimedOut,

    /// A streamed body went silent, or arrived slower than its route's minimum rate.
    #[error(
        code = "API_022",
        message = "The upload was too slow and was stopped. Try again on a steadier connection",
        developer_message = "a streamed body {reason}",
        status = 408,
        retryable
    )]
    BodyTooSlow {
        /// `went idle` or `fell below its minimum rate`.
        reason: &'static str,
    },

    /// As many bodies as a streamed route reads at once are already being read here.
    #[error(
        code = "API_023",
        message = "This service is busy. Try again in a moment",
        status = 503,
        retryable
    )]
    StreamBusy,

    /// The request was addressed to a host this server does not serve.
    #[error(
        code = "API_024",
        message = "This server does not serve that address",
        status = 421
    )]
    MisdirectedRequest,

    /// A streamed route has a body-reading layer in front of it, which it never hands a body.
    #[error(
        code = "API_025",
        message = "Body-reading middleware guards streamed routes: {routes}",
        status = 500
    )]
    StreamedRouteReadsBody {
        /// Every such route, as `METHOD /template`.
        routes: String,
    },

    /// The client stopped sending a body before it ended.
    #[error(
        code = "API_026",
        message = "The upload did not arrive completely. Try again",
        status = 400,
        retryable
    )]
    BodyInterrupted,

    /// A `multipart/form-data` body could not be read as the form a route accepts.
    #[error(
        code = "API_027",
        message = "The form could not be read",
        developer_message = "a multipart body was refused: {reason}",
        status = 400
    )]
    MalformedForm {
        /// What was wrong with it. A fixed phrase.
        reason: &'static str,
    },

    /// Path segments were declared in a way the router cannot honour.
    #[error(
        code = "API_028",
        message = "Path segments are declared inconsistently: {problems}",
        status = 500
    )]
    SegmentsMisdeclared {
        /// Every problem, `; `-separated.
        problems: String,
    },

    /// A read on a cross-origin route provoked by a page outside its origin set ([`guard_cross_origin_read`]).
    ///
    /// [`guard_cross_origin_read`]: crate::guard_cross_origin_read
    #[error(
        code = "API_031",
        message = "Cross-site read refused: this route answers its own origins only (Sec-Fetch-Site: {site}, Origin: {origin})",
        status = 403
    )]
    CrossSiteRead {
        /// The `Sec-Fetch-Site` the request carried, as a class.
        site: FetchSite,
        /// The `Origin` the request carried, parsed or classed — never the header.
        origin: ObservedOrigin,
    },

    /// A cross-origin route can answer no one: its origin set is empty, or it needs a principal it never reads.
    #[error(
        code = "API_030",
        message = "Routes declared cross-origin can answer no one: {problems}",
        status = 500
    )]
    CrossOriginMisdeclared {
        /// Every problem, `; `-separated.
        problems: String,
    },

    /// Two routes of one method match exactly the same paths.
    #[error(
        code = "API_029",
        message = "{method} {path} matches the same paths as {existing}",
        status = 500
    )]
    RouteAmbiguous {
        /// The method both serve.
        method: String,
        /// The template being registered.
        path: String,
        /// The template already registered.
        existing: String,
    },

    /// A path was declared to be answered before admission, and the router holds nothing there.
    #[error(
        code = "API_032",
        message = "Nothing is served at {path} to answer before admission",
        status = 500
    )]
    NothingToAnswerEarly {
        /// The path, or the directory ending in `/`, that was declared.
        path: String,
    },
    /// A page called a mounted action in process at a path its route does not serve.
    #[error(
        code = "API_034",
        message = "An action was called at a path that is not its own",
        developer_message = "{path} does not match the action route {template}",
        status = 500
    )]
    ActionPathMismatch {
        /// The path the action was called at.
        path: String,
        /// The action route's template.
        template: String,
    },
    /// The socket the service manager passed for a listener cannot be served.
    #[error(
        code = "API_033",
        message = "The activated socket {name} cannot be served: {reason}",
        status = 500
    )]
    ActivatedSocketUnavailable {
        /// The socket's name.
        name: String,
        /// Why.
        reason: String,
    },
}

impl ServerError {
    /// Creates a [`ServerError::Deserialize`] from any displayable value.
    pub fn deserialize(message: impl Into<String>) -> Self {
        Self::Deserialize(message.into())
    }

    /// Creates a [`ServerError::Serialize`] from any displayable value.
    pub fn serialize(message: impl Into<String>) -> Self {
        Self::Serialize(message.into())
    }

    /// Creates a [`ServerError::Internal`] from any displayable value.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    #[test]
    fn codes_and_statuses_are_stable() {
        let conflict = ServerError::RouteConflict {
            method: "GET".into(),
            path: "/u".into(),
        };
        assert_eq!(conflict.error_code(), "API_001");
        assert_eq!(conflict.http_status_code(), 500);
        assert!(!conflict.is_retryable());

        let not_found = ServerError::NotFound {
            method: "GET".into(),
            path: PathShape::new("/missing", |_| true),
        };
        assert_eq!(not_found.error_code(), "API_002");
        assert_eq!(not_found.http_status_code(), 404);

        let unsupported = ServerError::UnsupportedMethod;
        assert_eq!(unsupported.error_code(), "API_003");
        assert_eq!(unsupported.http_status_code(), 405);

        let not_allowed = ServerError::MethodNotAllowed {
            method: "DELETE".into(),
            path: PathShape::new("/u", |_| true),
        };
        assert_eq!(not_allowed.error_code(), "API_004");
        assert_eq!(not_allowed.http_status_code(), 405);

        assert_eq!(
            ServerError::MissingPathParam { name: "id".into() }.http_status_code(),
            500
        );
        assert_eq!(
            ServerError::InvalidPathParam {
                name: "id".into(),
                message: "not a number".into(),
            }
            .http_status_code(),
            400
        );
        assert_eq!(ServerError::deserialize("bad").http_status_code(), 400);
        assert_eq!(ServerError::serialize("bad").http_status_code(), 500);
    }

    #[test]
    fn bind_is_retryable() {
        let err = ServerError::Bind {
            address: "127.0.0.1:80".into(),
            message: "permission denied".into(),
        };
        assert_eq!(err.error_code(), "API_010");
        assert_eq!(err.http_status_code(), 503);
        assert!(err.is_retryable());
    }

    #[test]
    fn unauthenticated_constructor_is_generated_from_the_401_variant() {
        let err = ServerError::unauthenticated();
        assert_eq!(err.error_code(), "API_009");
        assert_eq!(err.http_status_code(), 401);
        assert!(matches!(err, ServerError::Unauthenticated(_)));
    }

    #[test]
    fn messages_interpolate_their_fields() {
        let err = ServerError::NotFound {
            method: "POST".into(),
            path: PathShape::new("/widgets", |_| true),
        };
        let rendered = err.to_string();
        assert!(rendered.contains("POST"));
        assert!(rendered.contains("/widgets"));
    }

    #[test]
    fn a_not_found_message_carries_the_shape_not_the_path() {
        let err = ServerError::NotFound {
            method: "GET".into(),
            path: PathShape::new("/reset/jan@example.nl", |segment| segment == "reset"),
        };
        let rendered = err.to_string();
        assert!(rendered.contains("/reset/{email}"), "{rendered}");
        assert!(!rendered.contains("jan"), "{rendered}");
    }

    #[test]
    fn a_cross_site_refusal_carries_classes_and_a_parsed_host() {
        let err = ServerError::CrossSiteWrite {
            site: FetchSite::parse(Some("jan@example.nl")),
            origin: ObservedOrigin::parse(Some("https://evil.example/jan@example.nl")),
        };
        assert_eq!(err.error_code(), "API_015");
        assert_eq!(err.http_status_code(), 403);
        let rendered = err.to_string();
        assert!(
            rendered.contains("Sec-Fetch-Site: unrecognised"),
            "{rendered}"
        );
        assert!(rendered.contains("Origin: malformed"), "{rendered}");
        assert!(!rendered.contains("jan"), "{rendered}");
    }
}
