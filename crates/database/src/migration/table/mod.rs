//! Schema building blocks for table definitions, independent of any backend.

mod check_constraint;
mod column;
mod foreign_key;
mod foreign_key_action;
mod index;
mod key_part;
mod retired;

pub use check_constraint::TableCheckConstraint;
pub use column::TableColumn;
pub use foreign_key::TableForeignKey;
pub use foreign_key_action::ForeignKeyAction;
pub use index::TableIndex;
pub(crate) use index::condition_words;
pub use key_part::{KeyPart, Nulls};
pub use retired::Retired;
