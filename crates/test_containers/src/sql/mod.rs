//! The YSQL fixtures: one shared node, and a store per test that nothing else can see.
//!
//! [`SqlSchema`] is the default; [`host_database_url`] is for a test that mints its own
//! namespaces; [`SqlDatabase`] is for a test whose subject is a database.

/// The machine-wide lock the **creates** are taken under.
///
/// `CREATE DATABASE` writes the cluster-wide catalog. The drops stay outside it, and like every
/// [`AcquireLock`](crate::acquire_lock::AcquireLock) it degrades to no lock, so the creates
/// pair it with an ask-then-create loop.
pub(super) const CATALOG_LOCK: &str = "sql-create-database";

mod ddl_turn;
mod host_database_url;
mod sql_auth_node;
mod sql_database;
mod sql_node;
mod sql_role;
mod sql_schema;
mod sweep_every_database;
mod sweep_stale_databases;

pub use ddl_turn::DdlTurn;
pub use host_database_url::host_database_url;
pub use sql_auth_node::SqlAuthNode;
pub use sql_database::SqlDatabase;
pub use sql_node::SqlNode;
pub use sql_role::SqlRole;
pub use sql_schema::SqlSchema;
