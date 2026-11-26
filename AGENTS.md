# Working on Fundamental

Fundamental is a Rust framework. Its public API is a product, and every exported item is a
promise. The order of priorities is **correct → transparent → stable → performant →
convenient**: a small, coherent, observable, well-tested surface over fast feature output.
When unsure, pick the safer option and say why.

Research and choose designs, error strategies and dependencies rather than taking the first
idea that compiles, and say briefly why in the pull request.

## Hard rules

1. **No `unsafe`.** Every crate root carries `#![forbid(unsafe_code)]`. Dependencies may use
   `unsafe` internally; the ban is on this repository's code.
2. **Stable Rust only.** No nightly, no `#![feature(...)]`, no unstable APIs. Edition 2024.
3. **A clean build is the bar.** A change is done when `cargo fmt`,
   `cargo clippy --workspace --all-targets -- -D warnings` and `cargo nextest run --workspace`
   pass with no warnings. Doctests run separately with `cargo test --doc`, since nextest does
   not run them.
4. **No silent failure.** Every error, fallback, retry and deliberately ignored value is logged
   with its context. A swallowed error is a bug.

## Observability

An engineer who has never seen the source can diagnose any failure from the logs alone. When
that takes reading the code, the missing instrumentation is the bug.

- Every public operation and every fallible internal step has a span
  (`#[instrument]`) carrying its key inputs as structured fields.
- Events are structured: `info!(user_id, count, "batch done")`, never values baked into the
  message.
- Levels: `error!` an operation failed and needs attention; `warn!` degraded but recovered;
  `info!` state transitions and lifecycle; `debug!` internals; `trace!` fine detail.
- Log which branch was taken and the values that chose it: a retry, a cache miss, a fallback.
- An error is logged once, where it is handled, with its whole `source` chain.
- A library emits and never configures: no crate installs a global subscriber.
  `tracing-subscriber` is a dev-dependency only.
- Never log secrets or personal data. Hold a secret in `fundamental::core::secrets::SecretString`,
  which redacts on `Debug` and `Display` and yields its value only through `.expose()`.

## Errors

Fundamental owns its error story: use its `#[derive(Error)]`, never `thiserror` or `anyhow`.
The derive covers the `thiserror` surface (`#[error("...")]`, `#[source]`, `#[from]`,
`transparent`), and every error also carries a stable `code`, a user-safe `message`, an HTTP
`status`, a `retryable` flag and an `exit_code`, so one value drives an API response, a log
line and a CLI exit.

- One matchable enum per fallible boundary, each variant with its metadata:
  `#[error(code = "DB_003", message = "Query failed", status = 500, retryable)]`.
- Keep the chain with `#[source]` or `#[from]`; never discard a cause.
- Group cases callers handle the same way; forward with `#[error(transparent)]`.
- When a variant's status or code depends on a classification, model it with
  `#[derive(ErrorOverride)]` and an `#[overrides]` field rather than one variant per case.
- Convert between error enums with `#[error_into(Target)]` and an `Other(AnyError)` catch-all,
  which keeps code, message, status, retryable and the chain.
- Public error enums are `#[non_exhaustive]`.
- No `unwrap`, `expect` or `panic!` on a reachable path. A panic is allowed only for a
  programmer-error invariant, documented in a `# Panics` section.

## The framework's own constructs first

- When Fundamental covers the job, use it: errors (`#[derive(Error)]`, `#[derive(ErrorOverride)]`,
  `AnyError`), validation (`#[derive(Validate)]`), typed ids (`#[derive(Identifier)]`), secrets
  (`SecretString`), configuration (`fundamental_config`), localization (`fundamental_i18n`,
  MessageFormat 2 messages) and persistence (`#[derive(Model)]`, repositories).
- A new external capability is wrapped behind one small Fundamental API, so every caller goes
  through one seam that is instrumented once and swapped in one place. `tracing`, `tokio`,
  `serde`, `clap` and the test tooling are used directly.

## Design

- Choose a pattern deliberately, and name the one rejected and why.
- Make illegal states unrepresentable: newtypes, typestate, closed enums. A `UserId` is not a
  `PostId`.
- Prefer static dispatch: generics and `impl Trait`; `dyn` only for runtime polymorphism.
- Search before building: `rg` the workspace for what exists, then reuse or extend it.
- When a request would duplicate, leak an abstraction, bloat the API or hurt observability,
  say so and propose an alternative before building it.
- Keep the surface small: `pub(crate)` by default. Every `pub` item has a doc comment opening
  with one short sentence, and `# Errors` and `# Panics` sections where they apply.

## Performance and stability

- Iterators, generics and borrowed parameters (`&str`, `&[T]`, `impl AsRef<_>`) over boxing,
  cloning and owned arguments.
- No optimisation without a `criterion` benchmark that shows it matters.
- A change to a hot path (rendering, action dispatch, query building, migration generation,
  the sync wire, configuration loading, message formatting) runs `fun bench`, which checks its
  instruction counts against `benches/baseline/<arch>.json`. When a count moves on purpose,
  `fun bench --save` rewrites the file, and the commit that changes it says why. A new hot path
  gets a workload in `fundamental_bench::workloads` and a bench in both `instructions.rs` and
  `wall.rs`.
- A change to how the app survives load or failure (pools, retries, reconnects, the job
  worker, sync) runs `fun bench load`, `fun bench soak` and `fun bench fault`, and updates the
  numbers in `BENCHMARKS.md` it moved. [`BENCHMARKS.md`](BENCHMARKS.md) holds the method, the
  machine and every command.
- Public changes follow semver: `#[non_exhaustive]` on what may grow, `#[must_use]` on
  builders and meaningful return values.
- No global mutable state; one-time initialisation uses `OnceLock`.

## One thing per file

- A file holds one public `struct`, `enum` or free `fn`, named as its `snake_case`
  (`ConnectionPool` → `connection_pool.rs`), with every `impl` of that type.
- A type's own error, builder or iterator may share its file; private helpers go below the
  impls; unit tests go at the bottom in `#[cfg(test)] mod tests`.
- A `mod.rs` holds only `mod` declarations and `pub use` re-exports.

## Imports

Bring items into scope with `use` and call them by their bare name: `use tracing::warn;` then
`warn!(...)`, never `tracing::warn!(...)` inline. Qualify only to separate two items of the
same name.

## Tests

- Every public item is tested on its happy path and its failure and edge paths: empty,
  boundary, invalid input, each error variant.
- Each test asserts one meaningful behaviour, and asserts the specific error variant, with its
  `error_code()` and `http_status_code()` where those are part of the contract.
- Where the contract includes observability, assert the events and spans with a test
  subscriber.

## Documentation and changelog

A change is not done until its docs and its changelog entry are in the same commit.

- What an item is and how to call it lives in its `///` comment; a crate's purpose in its
  `//!` header.
- Examples are ` ```rust ` doctests that compile and run; never ` ```ignore `, and
  ` ```no_run ` only when a live resource is needed.
- Add a `CHANGELOG.md` entry under `## [Unreleased]` (Added, Changed, Fixed, Removed) for any
  user-visible change. A breaking change says what to do instead.

## Before a pull request

```bash
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo test --doc --workspace --all-features
```

## Commits

- Every commit is `type(scope): description`: one line, at most 72 characters, lower case,
  imperative, no trailing period. `type` is one of
  `feat fix docs style refactor perf test build ci chore revert` (`!` marks a breaking change);
  `scope` is optional and comes from `.githooks/scopes`.
- A commit message is that one line and its sign-off: no body, no other trailers. Every commit
  is signed off under the [Developer Certificate of Origin](DCO) with `git commit -s`.
- `git config core.hooksPath .githooks` arms the hook that holds a commit to this rule.
