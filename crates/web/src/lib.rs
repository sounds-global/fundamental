//! Server-rendered web surface for the [Fundamental](https://docs.rs/fundamental) framework.
//!
//! A surface can call only the actions it lists in `mount_actions!`:
//!
//! ```compile_fail
//! use fundamental_core::AnyError;
//! use fundamental_macros::{action, mount_actions};
//! use fundamental_web::HttpContext;
//!
//! #[action(POST, "/api/greet")]
//! async fn greet(_c: HttpContext<()>, name: String) -> Result<String, AnyError> { Ok(name) }
//!
//! #[action(POST, "/api/farewell")]
//! async fn farewell(_c: HttpContext<()>, name: String) -> Result<String, AnyError> { Ok(name) }
//!
//! mount_actions! { Greet } // only `Greet` is mounted on this surface
//!
//! // `Farewell` is not mounted, so the `actions` module has no `farewell` stub:
//! let _unmounted = actions::farewell;
//! ```
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod prelude;

#[cfg(feature = "live")]
pub mod live;

// Compiled in both render modes: an island renders on the server before it hydrates.
#[cfg(any(feature = "ssr", feature = "hydrate"))]
mod action_client;
#[cfg(any(feature = "ssr", feature = "hydrate"))]
mod action_request;
#[cfg(any(feature = "ssr", feature = "hydrate"))]
mod web_client_error;

#[cfg(any(feature = "ssr", feature = "hydrate"))]
pub use action_client::{WebActionTransport, call_action};
#[cfg(any(feature = "ssr", feature = "hydrate"))]
pub use action_request::ActionRequest;
#[cfg(any(feature = "ssr", feature = "hydrate"))]
pub use web_client_error::{RemoteFault, WebClientError};

#[cfg(feature = "hydrate")]
mod document_locale;
#[cfg(feature = "hydrate")]
mod hydrate;
#[cfg(feature = "hydrate")]
mod hydrate_with;
#[cfg(feature = "hydrate")]
mod prefetch;
#[cfg(feature = "hydrate")]
mod scroll;
#[cfg(feature = "hydrate")]
pub use document_locale::document_locale;
#[cfg(feature = "hydrate")]
pub use hydrate::hydrate;
#[cfg(feature = "hydrate")]
pub use hydrate_with::hydrate_with;
#[cfg(feature = "hydrate")]
pub use prefetch::prefetch_links;
#[cfg(feature = "hydrate")]
pub use scroll::scroll_to_top;

#[cfg(any(feature = "ssr", feature = "hydrate"))]
pub use fundamental_core::escape;

#[cfg(any(feature = "ssr", feature = "hydrate"))]
mod hooks;
#[cfg(any(feature = "ssr", feature = "hydrate"))]
pub use hooks::*;

#[cfg(any(feature = "ssr", feature = "hydrate"))]
mod async_ui;
#[cfg(any(feature = "ssr", feature = "hydrate"))]
#[doc(hidden)]
pub use async_ui::Writes;
#[cfg(any(feature = "ssr", feature = "hydrate"))]
pub use async_ui::{
    ActionHook, AsyncDefaults, AsyncDefaultsConfig, AsyncErrorView, AsyncSuccessView, Mutation,
    MutationScope, MutationSource, Query, QuerySource,
};

#[cfg(all(test, target_arch = "wasm32"))]
mod bootstrap_refuses_a_planted_twin;
#[cfg(feature = "ssr")]
mod default_document;
#[cfg(feature = "ssr")]
mod document;
#[cfg(feature = "ssr")]
mod hydration_bootstrap;
#[cfg(feature = "ssr")]
mod hydration_script;
#[cfg(feature = "ssr")]
mod hydration_script_csp_hash;
#[cfg(feature = "ssr")]
mod share_card;
#[cfg(feature = "ssr")]
mod site_icon;

#[cfg(any(feature = "ssr", feature = "hydrate"))]
mod page_context;

#[cfg(any(feature = "ssr", feature = "hydrate"))]
mod module_script;
#[cfg(any(feature = "ssr", feature = "hydrate"))]
mod page_head;

#[cfg(feature = "ssr")]
mod in_language;
#[cfg(feature = "ssr")]
mod in_request_language;
#[cfg(feature = "ssr")]
mod page_data;
#[cfg(feature = "ssr")]
mod render;
#[cfg(feature = "ssr")]
mod render_options;
#[cfg(feature = "ssr")]
mod render_page;
#[cfg(feature = "ssr")]
mod render_page_stream;
#[cfg(feature = "ssr")]
mod render_page_under;
#[cfg(feature = "ssr")]
mod render_page_with;

#[cfg(any(feature = "ssr", feature = "hydrate"))]
mod page;
#[cfg(feature = "ssr")]
mod rendered_page;
#[cfg(feature = "ssr")]
mod web_server;

#[cfg(feature = "ssr")]
pub use default_document::default_document;
#[cfg(feature = "ssr")]
pub use document::Document;
#[cfg(feature = "ssr")]
pub use hydration_bootstrap::hydration_bootstrap;
#[cfg(feature = "ssr")]
pub use hydration_script::hydration_script;
#[cfg(feature = "ssr")]
pub use hydration_script_csp_hash::hydration_script_csp_hash;
#[cfg(feature = "ssr")]
pub use in_request_language::{InRequestLanguage, in_request_language};
#[cfg(any(feature = "ssr", feature = "hydrate"))]
pub use module_script::ModuleScript;
#[cfg(feature = "ssr")]
pub use page::{
    Cache, Granted, GuardState, Guarded, GuardedPolicy, Join, LoadCx, Loader, Loads, Open,
    PageError, PagePolicy, PagePolicyError, PagePolicyGuard, PageRefusal, PageRoute, PendingGuard,
    PolicyChain, Requirement, Sourced, at,
};
#[cfg(any(feature = "ssr", feature = "hydrate"))]
pub use page::{GateReason, PublicSegment, Refused};
#[cfg(any(feature = "ssr", feature = "hydrate"))]
pub use page_context::PageContext;
#[cfg(feature = "ssr")]
pub use page_data::PageData;
#[cfg(any(feature = "ssr", feature = "hydrate"))]
pub use page_head::PageHead;
#[cfg(feature = "ssr")]
pub use render::render_to_html;
#[cfg(feature = "ssr")]
pub use render_page::render_page;
#[cfg(feature = "ssr")]
pub use render_page_stream::{render_page_stream, render_page_stream_with};
#[cfg(feature = "ssr")]
pub use render_page_under::render_page_under;
#[cfg(feature = "ssr")]
pub use render_page_with::render_page_with;
#[cfg(feature = "ssr")]
pub use rendered_page::RenderedPage;
#[cfg(feature = "ssr")]
pub use share_card::ShareCard;
#[cfg(feature = "ssr")]
pub use site_icon::SiteIcon;
#[cfg(feature = "ssr")]
pub use web_server::WebServer;

/// A running web surface's listener, returned by [`WebServer::serve`].
#[cfg(feature = "ssr")]
pub use fundamental_api::ApiListener as WebListener;

/// The HTTP construction types a web surface is built from.
#[cfg(feature = "ssr")]
pub use fundamental_server::{
    ContentSecurityPolicy, CredentialSource, CspHash, CspSource, FrameAncestors, FrameSource, Hsts,
    HstsMaxAge, HttpContext, Indexing, Locale, LocalePolicy, LocaleSource, Method, OpenerPolicy,
    PathParams, PreferencePolicy, ReferrerPolicy, Route, Router, SecurityHeaders, ServerConfig,
    ServerError, ServerListener, StaticAsset, StaticSource,
};

/// The address vocabulary a market-addressed surface is built from.
///
/// Not gated on `ssr`: a page file names these and compiles into both bundles.
pub use fundamental_core::address::{
    AbsoluteUrl, AddressError, AddressSpace, LocalePrefixSet, PageLanguage, PageSlug, PrefixScope,
    RequestAddress, ResolvedAddress, SlugStatus, SlugTable, SlugTableBuilder, WebOrigin,
};
