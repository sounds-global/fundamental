#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::all)]

pub use fundamental_core::{
    AnyError, CommandAddress, CommandAddressObjection, Connection, Error, ErrorOverride,
    GeneratedId, Handle, HasHandle, Identifier, Model, Primary, ScopeSet, Validate, Variant,
    action::Action,
};
// `#[macro_export]` puts `scopes!` at its crate root, so `core` below does not carry it.
pub use fundamental_core::scopes;
pub use fundamental_macros::*;

/// The market-addressed URL vocabulary: one canonical address per page and locale.
pub use fundamental_core::address;

/// The locale an [`address`] prefix selects.
pub use fundamental_core::i18n::Locale;

/// Exact money: [`Money`](money::Money) in millionths.
pub use fundamental_core::money;

/// Property tests that run for a duration from a replayable seed.
#[cfg(feature = "proptest")]
#[cfg_attr(docsrs, doc(cfg(feature = "proptest")))]
pub use fundamental_core::testing;

/// Collections bounded by their type.
pub use fundamental_core::bounded;

/// How a signing key is held.
pub use fundamental_core::custody;

/// Lets the derive macros resolve `fundamental_core` paths through the umbrella.
#[doc(hidden)]
pub use fundamental_core as core;

/// Structural type reflection behind [`Schema`](macro@Schema). See [`fundamental_schema`].
#[cfg(feature = "schema")]
#[cfg_attr(docsrs, doc(cfg(feature = "schema")))]
pub use fundamental_schema as schema;

/// Canonical bytes and the digests that name them. See [`fundamental_canonical`].
#[cfg(feature = "canonical")]
#[cfg_attr(docsrs, doc(cfg(feature = "canonical")))]
pub use fundamental_canonical as canonical;

/// Confusable skeletons: what a name looks like. See [`fundamental_text`].
#[cfg(feature = "text")]
#[cfg_attr(docsrs, doc(cfg(feature = "text")))]
pub use fundamental_text as text;

/// Embed a canonical document checked at build time. See [`fundamental_canonical_macros`].
#[cfg(feature = "canonical")]
#[cfg_attr(docsrs, doc(cfg(feature = "canonical")))]
pub use fundamental_canonical_macros::embed_canonical;

/// Derive a struct's one RFC 8785 encoding. See [`fundamental_canonical_macros::CanonicalJson`].
#[cfg(feature = "canonical")]
#[cfg_attr(docsrs, doc(cfg(feature = "canonical")))]
pub use fundamental_canonical_macros::CanonicalJson;

/// Dev-only OpenAPI and AsyncAPI of the app's surface. See [`fundamental_introspect`].
#[cfg(feature = "introspect")]
#[cfg_attr(docsrs, doc(cfg(feature = "introspect")))]
pub use fundamental_introspect as introspect;

/// The project's manifest, read through one loader. See [`fundamental_manifest`].
#[cfg(feature = "manifest")]
#[cfg_attr(docsrs, doc(cfg(feature = "manifest")))]
pub use fundamental_manifest as manifest;

/// Type-safe, multi-source configuration. See [`fundamental_config`].
#[cfg(feature = "config")]
#[cfg_attr(docsrs, doc(cfg(feature = "config")))]
pub use fundamental_config as config;

/// The loaded settings of the active app. See [`fundamental_config::AppSettings`].
#[cfg(feature = "config")]
#[cfg_attr(docsrs, doc(cfg(feature = "config")))]
pub use fundamental_config::AppSettings;

/// Abstract, compile-time-safe database traits. See [`fundamental_db`].
#[cfg(feature = "db")]
#[cfg_attr(docsrs, doc(cfg(feature = "db")))]
pub use fundamental_db as db;

/// The relational adapter, over the PostgreSQL wire. See [`fundamental_sql`].
#[cfg(feature = "sql")]
#[cfg_attr(docsrs, doc(cfg(feature = "sql")))]
pub use fundamental_sql as sql;

/// The wide-column adapter, over the Cassandra wire. See [`fundamental_cql`].
#[cfg(feature = "cql")]
#[cfg_attr(docsrs, doc(cfg(feature = "cql")))]
pub use fundamental_cql as cql;

/// Offline SQLite storage (IndexedDB-backed on `wasm32`). See [`fundamental_offline`].
#[cfg(feature = "offline")]
#[cfg_attr(docsrs, doc(cfg(feature = "offline")))]
pub use fundamental_offline as offline;

/// Field-level encryption with key rotation. See [`fundamental_encryption`].
#[cfg(feature = "encryption")]
#[cfg_attr(docsrs, doc(cfg(feature = "encryption")))]
pub use fundamental_encryption as encryption;

/// HashiCorp Vault client for leased credentials and Transit. See [`fundamental_secrets`].
#[cfg(feature = "secrets")]
#[cfg_attr(docsrs, doc(cfg(feature = "secrets")))]
pub use fundamental_secrets as secrets;

/// Model-driven migration formatting and execution. See [`fundamental_db_migrator`].
#[cfg(feature = "migrator")]
#[cfg_attr(docsrs, doc(cfg(feature = "migrator")))]
pub use fundamental_db_migrator as migrator;

/// Type-safe NATS messaging: pub/sub, request-reply, JetStream. See [`fundamental_nats`].
#[cfg(feature = "nats")]
#[cfg_attr(docsrs, doc(cfg(feature = "nats")))]
pub use fundamental_nats as nats;

/// WebSocket server: action-routed, typed message handlers. See [`fundamental_ws`].
#[cfg(feature = "ws")]
#[cfg_attr(docsrs, doc(cfg(feature = "ws")))]
pub use fundamental_ws as ws;

/// Database seeding through [`#[seed]`](macro@seed). See [`fundamental_seed`].
#[cfg(feature = "seed")]
#[cfg_attr(docsrs, doc(cfg(feature = "seed")))]
pub use fundamental_seed as seed;

/// Developer commands through [`#[command]`](macro@command). See [`fundamental_command`].
#[cfg(feature = "command")]
#[cfg_attr(docsrs, doc(cfg(feature = "command")))]
pub use fundamental_command as command;

/// Offline sync for `#[model(syncable)]` models. See [`fundamental_sync`].
#[cfg(feature = "sync")]
#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub use fundamental_sync as sync;

/// Device-side sync transport over one WebSocket. See [`fundamental_ws_client`].
#[cfg(feature = "ws_client")]
#[cfg_attr(docsrs, doc(cfg(feature = "ws_client")))]
pub use fundamental_ws_client as ws_client;

/// Device identity: per-device ed25519 keys in platform secure storage. See [`fundamental_keys`].
#[cfg(feature = "keys")]
#[cfg_attr(docsrs, doc(cfg(feature = "keys")))]
pub use fundamental_keys as keys;

/// Principal authentication: a JWT verified against JWKS. See [`fundamental_auth`].
#[cfg(feature = "auth")]
#[cfg_attr(docsrs, doc(cfg(feature = "auth")))]
pub use fundamental_auth as auth;

/// Native relationship-based authorization. See [`fundamental_authz`].
#[cfg(feature = "authz-schema")]
#[cfg_attr(docsrs, doc(cfg(feature = "authz-schema")))]
pub use fundamental_authz as authz;

/// Server-side sync endpoint over WebSocket. See [`fundamental_sync_server`].
#[cfg(feature = "sync_server")]
#[cfg_attr(docsrs, doc(cfg(feature = "sync_server")))]
pub use fundamental_sync_server as sync_server;

/// Builds a supervised [`LiveSyncSurface`]. See [`fundamental_sync_server::LiveSyncServer`].
#[cfg(feature = "sync_server")]
#[cfg_attr(docsrs, doc(cfg(feature = "sync_server")))]
pub use fundamental_sync_server::LiveSyncServer;

/// The filesystem-to-routes build engine. See [`fundamental_build`].
#[cfg(feature = "build")]
#[cfg_attr(docsrs, doc(cfg(feature = "build")))]
pub use fundamental_build as build;

/// Framework-neutral HTTP server core. See [`fundamental_server`].
#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub use fundamental_server as server;

/// HTTP API surface backed by axum. See [`fundamental_api`].
#[cfg(feature = "api")]
#[cfg_attr(docsrs, doc(cfg(feature = "api")))]
pub use fundamental_api as api;

/// Server-rendered web surface with `#[island]` interactivity. See [`fundamental_web`].
#[cfg(any(feature = "web", feature = "web-hydrate"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "web", feature = "web-hydrate"))))]
pub use fundamental_web as web;

/// Native (Tauri) surface seam. See [`fundamental_native`].
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "native", not(target_arch = "wasm32")))))]
pub use fundamental_native as native;

/// Surface supervisor: surfaces under one shutdown signal. See [`fundamental_runtime`].
#[cfg(feature = "runtime")]
#[cfg_attr(docsrs, doc(cfg(feature = "runtime")))]
pub use fundamental_runtime as runtime;

/// Singleton work behind a fencing lease. See [`fundamental_lease`].
#[cfg(feature = "lease")]
#[cfg_attr(docsrs, doc(cfg(feature = "lease")))]
pub use fundamental_lease as lease;

/// Durable jobs due at a time. See [`fundamental_jobs`].
#[cfg(feature = "jobs")]
#[cfg_attr(docsrs, doc(cfg(feature = "jobs")))]
pub use fundamental_jobs as jobs;

/// An idempotent-consumer inbox. See [`fundamental_inbox`].
#[cfg(feature = "inbox")]
#[cfg_attr(docsrs, doc(cfg(feature = "inbox")))]
pub use fundamental_inbox as inbox;

/// Process telemetry: one tracing subscriber. See [`fundamental_observability`].
#[cfg(feature = "observability")]
#[cfg_attr(docsrs, doc(cfg(feature = "observability")))]
pub use fundamental_observability as observability;

/// One app as one value. See [`fundamental_app`].
#[cfg(feature = "app")]
#[cfg_attr(docsrs, doc(cfg(feature = "app")))]
pub use fundamental_app as app;

/// One app as one value. See [`fundamental_app::App`].
#[cfg(feature = "app")]
#[cfg_attr(docsrs, doc(cfg(feature = "app")))]
pub use fundamental_app::App;

/// [`App::new`] for the calling crate. See [`fundamental_app::app!`].
#[cfg(feature = "app")]
#[cfg_attr(docsrs, doc(cfg(feature = "app")))]
pub use fundamental_app::app;

/// Supervises surfaces under one shutdown signal. See [`fundamental_runtime::Runtime`].
#[cfg(feature = "runtime")]
#[cfg_attr(docsrs, doc(cfg(feature = "runtime")))]
pub use fundamental_runtime::Runtime;

/// A live slice-sync server as a supervised surface. See [`fundamental_runtime::LiveSyncSurface`].
#[cfg(all(feature = "runtime", feature = "ws"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "runtime", feature = "ws"))))]
pub use fundamental_runtime::LiveSyncSurface;

/// A crate's MessageFormat 2 messages, said in the reader's language.
#[cfg(feature = "messages")]
#[cfg_attr(docsrs, doc(cfg(feature = "messages")))]
pub use fundamental_messages as messages;

/// Localization: catalogs, locale negotiation and the translator. See [`fundamental_i18n`].
#[cfg(feature = "i18n")]
#[cfg_attr(docsrs, doc(cfg(feature = "i18n")))]
pub mod i18n {
    pub use fundamental_i18n::*;
}
