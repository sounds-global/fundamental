//! Reading migration DDL well enough to guard it, and no further.
//!
//! Not a SQL parser: it recognises the shapes the guards judge (keys, destructive verbs, added
//! columns, `CHECK … IN (…)` bounds) and calls everything else [`DdlStatement::Other`]. A
//! `CREATE TABLE` or `CREATE INDEX` it cannot read is a [`DdlError`], never a pass. An
//! undeclared key is not unset: the engine hashes its leading column.

mod ddl_column;
mod ddl_error;
mod ddl_statement;
mod key_declaration;
mod key_element;
mod recognise;
mod rerun;
mod sharding;
mod split_top_level;

pub use ddl_column::DdlColumn;
pub use ddl_error::DdlError;
pub use ddl_statement::DdlStatement;
pub use key_declaration::KeyDeclaration;
pub use key_element::KeyElement;
pub use recognise::{recognise, recognise_all};
pub use rerun::why_not_rerunnable;
pub use sharding::Sharding;

pub(crate) use recognise::column_renames;
pub(crate) use split_top_level::split_top_level;
