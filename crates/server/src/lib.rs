//! Framework-neutral HTTP server core for the [Fundamental](https://docs.rs/fundamental) framework.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

// `#[middleware]` refers to `::fundamental_server`, which this alias resolves inside the crate.
extern crate self as fundamental_server;

mod accept_language;
mod action_call;
mod action_descriptor;
mod action_dispatcher;
mod action_meta;
mod address_stylesheet;
mod admission;
mod admission_request;
mod audience;
mod auth_middleware;
mod authorized_objects;
mod body;
mod body_policy;
mod bundle_stamps;
mod bytes_handler;
mod bytes_io_handler;
mod cookie_lookup;
mod cookie_write_guard;
mod credential_source;
mod cross_origin_read_guard;
mod dev_tools_origin;
mod error_page;
mod extensions;
mod fetch_site;
#[cfg(feature = "multipart")]
mod file_part;
#[cfg(feature = "multipart")]
mod form_file;
mod from_ref;
mod host_cookie;
mod http_context;
mod http_handler;
mod if_none_match;
mod inbound_request_id;
mod inbound_trace_context;
mod language_cookie;
mod language_verdict;
mod live_reload;
mod locale_choice;
mod locale_policy;
mod locale_source;
mod login_route;
mod method;
mod middleware;
mod next;
mod observed_origin;
mod page_auth;
mod page_verdict;
mod path_params;
mod path_shape;
mod peer;
mod preference_policy;
mod presented_credential;
mod projected_handler;
mod public_route;
mod raw_handler;
mod raw_input_handler;
mod redirect;
mod request_body_slot;
mod request_body_stream;
mod request_headers;
mod request_id_provenance;
mod request_id_resolution;
mod request_return_path;
mod resolved_segment;
mod response_headers;
mod response_redirect;
mod response_status;
mod response_stream_slot;
mod route;
mod route_pattern;
mod router;
mod same_site;
mod scope;
mod security_headers;
mod segment_ledger;
mod segment_resolution;
mod segment_resolver;
mod segment_stage;
mod server;
mod server_action;
mod server_backend;
mod server_config;
mod server_error;
mod server_request;
mod server_response;
mod static_asset;
mod static_asset_content;
mod static_source;
mod stream_bounds;
mod stream_permit;
#[cfg(feature = "multipart")]
mod streamed_form;
mod streamed_handler;
mod streamed_response;
mod streaming_response_handler;
mod template_params;

pub use action_descriptor::ActionDescriptor;
pub use action_dispatcher::ActionDispatcher;
pub use action_meta::ActionMeta;
pub use admission::Admission;
pub use admission_request::AdmissionRequest;
pub use auth_middleware::auth;
pub use authorized_objects::AuthorizedObjects;
pub use body::{decode_body, encode_body};
pub use body_policy::BodyPolicy;
pub use cookie_write_guard::guard_cookie_write;
pub use credential_source::CredentialSource;
pub use cross_origin_read_guard::guard_cross_origin_read;
pub use dev_tools_origin::{NATIVE_WEBVIEW_SCHEME, is_dev_tools_origin};
pub use fetch_site::FetchSite;
#[cfg(feature = "multipart")]
pub use file_part::FilePart;
#[cfg(feature = "multipart")]
pub use form_file::FormFile;
pub use host_cookie::HostCookie;
pub use request_body_stream::RequestBodyStream;
pub use same_site::SameSite;
pub use stream_bounds::StreamBounds;
pub use stream_permit::StreamPermit;
#[cfg(feature = "multipart")]
pub use streamed_form::StreamedForm;
pub use streamed_response::{INTERNAL_CSP_NONCE_HEADER, StreamedResponse};
pub use template_params::template_params;

#[doc(no_inline)]
pub use from_ref::FromRef;
pub use fundamental_schema::{SchemaNode, describe};
pub use http_context::HttpContext;
pub use inbound_request_id::InboundRequestId;
pub use inbound_trace_context::InboundTraceContext;

#[doc(no_inline)]
pub use fundamental_core::i18n::Locale;
pub use locale_policy::LocalePolicy;
pub use locale_source::LocaleSource;
pub use method::Method;
pub use middleware::{Middleware, MiddlewareResult};
pub use next::Next;
pub use observed_origin::ObservedOrigin;
pub use path_params::PathParams;
pub use path_shape::PathShape;
pub use peer::Peer;
pub use preference_policy::PreferencePolicy;
pub use presented_credential::PresentedCredential;
pub use redirect::Redirect;
pub use request_headers::RequestHeaders;
pub use request_id_provenance::RequestIdProvenance;
pub use resolved_segment::ResolvedSegment;
pub use segment_ledger::SegmentLedger;
pub use segment_resolution::SegmentResolution;
pub use segment_resolver::SegmentResolver;

pub use audience::Audience;
pub use fundamental_core::address::{ReturnPath, ReturnPathError};
pub use fundamental_observability::{RequestId, RequestIdRejection};
pub use language_cookie::LanguageCookie;
pub use language_verdict::LanguageVerdict;
pub use locale_choice::LocaleChoice;
pub use login_route::LoginRoute;
pub use page_auth::PageAuth;
pub use page_verdict::PageVerdict;
pub use public_route::PublicRoute;
pub use request_id_resolution::{REQUEST_ID_HEADER, RequestIdResolution};
pub use request_return_path::request_return_path;
pub use response_headers::ResponseHeaders;
pub use response_redirect::ResponseRedirect;
pub use response_status::ResponseStatus;
pub use route::Route;
pub use router::Router;
pub use scope::{Scope, scope};
pub use security_headers::{
    ContentSecurityPolicy, CspHash, CspSource, FrameAncestors, FrameSource, HeaderCombine, Hsts,
    HstsMaxAge, Indexing, OpenerPolicy, ReferrerPolicy, SecurityHeader, SecurityHeaders,
};
pub use server::Server;
pub use server_action::ServerAction;
pub use server_backend::{ServerBackend, ServerListener};
pub use server_config::ServerConfig;
pub use server_error::{ServerError, ServerResult};
pub use server_request::ServerRequest;
pub use server_response::ServerResponse;
pub use static_asset::StaticAsset;
pub use static_source::StaticSource;
