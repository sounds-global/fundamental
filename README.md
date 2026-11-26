<!--
  The front page of the `fundamental` crate's API docs, included by `src/lib.rs`. Every
  ```rust block is a doctest run by `cargo test --doc -p fundamental`.
-->

# Fundamental

A developer-friendly Rust framework for building production systems. Fundamental
gives you one consistent way to define domain models, typed identifiers, errors,
and validation; to persist them across YugabyteDB (its YSQL and YCQL APIs) and offline SQLite;
and to connect everything with configuration, localization, field-level
encryption, secrets, and NATS messaging. Each is a first-class, observable building
block rather than a bolt-on.

> Status: `0.1.0`, pre-1.0. The API may change between minor versions until 1.0.

## Why Fundamental

- **One model, many backends.** A `#[derive(Model)]` type declares its database
  affinity at compile time; operations cannot accidentally cross database boundaries.
- **Errors that carry their metadata.** `#[derive(Error)]` attaches a stable code,
  user message, HTTP status, and retryability to every variant.
- **Validation that collects everything.** `#[derive(Validate)]` reports *all*
  failures at once instead of stopping at the first.
- **Make illegal states uncompilable.** A growing derive suite pushes invariants
  into the type system: `#[derive(Identifier)]` for strongly-typed ids (a `UserId`
  is not a `PostId`), `#[derive(Message)]` for compile-time-validated NATS subjects,
  `#[derive(Messages)]` over MessageFormat 2 messages for translated text, and
  `#[derive(ErrorOverride)]` for runtime-refined error metadata.
- **Production concerns built in.** Connection pooling, migrations, NATS pub/sub and
  JetStream, multi-source configuration, localization, `tracing` instrumentation,
  Prometheus metrics, field encryption, and Vault-leased credentials ship as part of
  the framework, not as an afterthought.

## Installation

Fundamental is not on crates.io yet; depend on it from git:

```toml
[dependencies]
fundamental = { git = "https://github.com/sounds-global/fundamental" }
```

The `fun` CLI scaffolds and runs projects:

```sh
cargo install --git https://github.com/sounds-global/fundamental fundamental_cli
fun new acme
```

Fundamental targets Rust 2024 and a minimum supported Rust version (MSRV) of
**1.96**, raised only in a minor release.

## A first taste

Define an input type, validate it, and model a domain error, all from the umbrella
crate. This example is a doctest, compiled and run with the tests.

```rust
use fundamental::{Validate, Error};

/// What a client sends to create an account.
#[derive(Validate)]
struct CreateUser {
    #[validate(email)]
    email: String,
    #[validate(length(min = 8, max = 100))]
    password: String,
}

/// Errors this operation can return, each with a stable code and HTTP status.
#[derive(Debug, Error)]
enum SignupError {
    #[error(code = "SIGNUP_001", message = "The submitted data was invalid", status = 400)]
    Invalid,
}

// A well-formed request validates cleanly.
let ok = CreateUser { email: "ada@example.com".into(), password: "correct horse".into() };
assert!(ok.validate().is_ok());

// A bad request fails validation, reporting every problem, not just the first.
let bad = CreateUser { email: "not-an-email".into(), password: "short".into() };
assert!(bad.validate().is_err());
```

For database-backed models, see [`fundamental_db`] and the per-backend crates, whose
examples live next to the code that depends on the database drivers.

## Workspace layout

Fundamental is a Cargo workspace. The `fundamental` umbrella crate is the front
door; the building blocks live in focused crates you can also depend on directly:

| Crate | Purpose |
|-------|---------|
| [`fundamental_core`] | Traits + marker types (`Model`, `Error`, `Validate`); zero-dependency, `no_std`-capable |
| [`fundamental_macros`] | Derive macros (`Model`, `Error`, `ErrorOverride`, `Validate`, `Identifier`, `Messages`, `Message`, `Schema`, `Variant`, `Connection`, `Queue`, `Stream`, `Consumer`) |
| [`fundamental_schema`] | Structural type reflection: `#[derive(Schema)]` and a JSON-Schema-2020-12 model of serde wire types, feeding OpenAPI/AsyncAPI and API tooling |
| [`fundamental_db`] | Abstract, compile-time-safe database traits (repositories, transactions, migrations) |
| [`fundamental_sql`] | YugabyteDB YSQL adapter (distributed, PostgreSQL-compatible SQL) |
| [`fundamental_cql`] | YugabyteDB YCQL adapter (wide-column, Cassandra-compatible, high throughput) |
| [`fundamental_offline`] | Offline SQLite: native everywhere, IndexedDB-backed on `wasm32` |
| [`fundamental_config`] | Type-safe, multi-source configuration |
| [`fundamental_encryption`] | Field-level encryption with key rotation |
| [`fundamental_secrets`] | HashiCorp Vault client (leased DB credentials, Transit) |
| [`fundamental_i18n`] | Localization: the app's catalog of every crate's messages, locale negotiation, runtime translator |
| [`fundamental_messages`] | A crate's MessageFormat 2 messages as typed functions (`m::panel::save()`), said in the reader's language |
| [`fundamental_messages_build`] | A crate's messages built and checked: typed functions from the source language, each language held to the project's policy |
| [`fundamental_db_migrator`] | Model-driven migration formatting + execution |
| [`fundamental_nats`] | Type-safe NATS messaging: pub/sub, request-reply, JetStream |
| [`fundamental_ws`] | WebSocket server: action-routed, typed message handlers |
| [`fundamental_server`] | Framework-neutral HTTP server core: a typed router, request context, and the pluggable `ServerBackend` seam |
| [`fundamental_api`] | HTTP API surface: an axum-backed `ServerBackend`, behind the seam so no web-framework type reaches a handler |
| [`fundamental_web`] | Server-rendered web surface: SSR pages with opt-in `#[island]` interactivity, Leptos behind one seam |
| [`fundamental_native`] | Native (Tauri) surface: the same pages/actions in a desktop/mobile app — in-process SSR, no bound port, and a hosted offline database (`SyncHost`) syncing while the webview sleeps |
| [`fundamental_auth`] | Principal authentication: a token becomes a typed `AuthenticatedPrincipal` (bundled `JwtVerifier`) |
| [`fundamental_keys`] | Per-device identity: ed25519 signing keys held in platform-native secure storage |
| [`fundamental_sync`] | Offline-first device sync: the engine, the slice model, and the wire protocol |
| [`fundamental_sync_server`] | Server sync endpoint: slice-resolved, principal-scoped WebSocket sync, with reactive projection maintenance |
| [`fundamental_ws_client`] | Device WebSocket sync transport: one codebase for native and `wasm32` |
| [`fundamental_seed`] | Database seeding: the `#[seed]` attribute + an ordered runner |
| [`fundamental_command`] | Developer commands: the `#[command]` attribute + a name-dispatched runner behind `fun run` |
| [`fundamental_build`] | Framework-neutral filesystem → routes build engine (page scanner, link builders, migration/seed discovery) |
| [`fundamental_introspect`] | Dev-only surface introspection: exposes an app's `#[action]`/message surface as `surface.json` / OpenAPI 3.1 / AsyncAPI 3.0 plus a bounded log/trace drain under `/_fundamental` (enabled by `fun dev`, never in release) |
| [`fundamental_app`] | One `App` value per app: its pages, API, worker, socket and sync surfaces declared once, run by a one-line `main` (`fundamental::app!()` names it after its crate) and answered by its tests; which surfaces a process runs is configuration, never code |
| [`fundamental_runtime`] | Surface supervisor: run one or more surfaces under one shutdown signal |
| `fundamental_cli` (`fun`) | Developer CLI: scaffolding (`fun new`/`add`), the build/run loop for any app (`fun build`/`fun dev`), migrations, seeds, commands (`fun run`); `fun <name>` runs a `fun-<name>` program from `PATH` |

[`fundamental_core`]: crates/core
[`fundamental_macros`]: crates/macros
[`fundamental_schema`]: crates/schema
[`fundamental_introspect`]: crates/introspect
[`fundamental_db`]: crates/database
[`fundamental_sql`]: crates/sql
[`fundamental_cql`]: crates/cql
[`fundamental_offline`]: crates/offline
[`fundamental_config`]: crates/config
[`fundamental_encryption`]: crates/encryption
[`fundamental_secrets`]: crates/secrets
[`fundamental_i18n`]: crates/i18n
[`fundamental_messages`]: crates/messages
[`fundamental_messages_build`]: crates/messages_build
[`fundamental_db_migrator`]: crates/database_migrator
[`fundamental_nats`]: crates/nats
[`fundamental_ws`]: crates/ws
[`fundamental_server`]: crates/server
[`fundamental_api`]: crates/api
[`fundamental_web`]: crates/web
[`fundamental_native`]: crates/native
[`fundamental_auth`]: crates/auth
[`fundamental_keys`]: crates/keys
[`fundamental_sync`]: crates/sync
[`fundamental_sync_server`]: crates/sync_server
[`fundamental_ws_client`]: crates/ws_client
[`fundamental_seed`]: crates/seed
[`fundamental_command`]: crates/command
[`fundamental_build`]: crates/build
[`fundamental_app`]: crates/app
[`fundamental_runtime`]: crates/runtime

## Documentation

The API reference is generated by `rustdoc` from the code: build it with `cargo doc --open`.
[`AGENTS.md`](AGENTS.md) holds the rules a change to this repository follows, for people and
coding agents alike; [`CONTRIBUTING.md`](CONTRIBUTING.md) says how to propose one.
[`BENCHMARKS.md`](BENCHMARKS.md) shows what the public features cost and how they hold up:
instruction counts checked on every pull request, wall-clock times, load at rising concurrency,
a soak and fault runs on real engines, each with the command that reproduces it.

## License

Licensed under the [Apache License, Version 2.0](https://github.com/sounds-global/fundamental/blob/main/LICENSE). See [`NOTICE`](https://github.com/sounds-global/fundamental/blob/main/NOTICE).
