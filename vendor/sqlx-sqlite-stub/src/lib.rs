//! Version-resolution stub for `sqlx-sqlite` — **not** the real driver.
//!
//! `libsqlite3-sys` carries `links = "sqlite3"`, so cargo permits exactly one
//! version of it per dependency graph, and it resolves optional dependencies
//! even when no feature enables them. sqlx 0.9 (the Postgres driver behind
//! `fundamental_sql`) caps `libsqlite3-sys < 0.38` through its optional
//! `sqlx-sqlite` driver, while rusqlite 0.40 (`fundamental_offline`, SQLite
//! 3.53+) requires `libsqlite3-sys 0.38`. Those ranges cannot co-resolve.
//!
//! This workspace never enables sqlx's SQLite driver — SQLite belongs to
//! `fundamental_offline` via rusqlite — so a `[patch.crates-io]` entry in the
//! workspace root redirects `sqlx-sqlite` to this empty crate, which declares no
//! `libsqlite3-sys` dependency and lifts the constraint from resolution.
//!
//! This crate is a workspace member so that applications depending on
//! `fundamental` over git can reach it for the matching patch they must add
//! themselves (patch tables never propagate through a dependency). It compiles
//! to nothing. If sqlx's SQLite driver is ever actually enabled, sqlx references
//! real `sqlx_sqlite` items this crate does not define, so the build fails
//! loudly rather than silently linking a non-functional driver.
//!
//! Remove this stub and its `[patch.crates-io]` entry once sqlx widens its
//! `libsqlite3-sys` range to include 0.38:
//! <https://github.com/launchbadge/sqlx/blob/main/sqlx-sqlite/Cargo.toml>
