//! Reading a live SQL schema back into the diff engine's vocabulary, and one table's full catalog.

mod catalog_column;
mod catalog_constraint;
mod catalog_index;
mod introspector;
mod role_access;
mod table_catalog;

pub use catalog_column::CatalogColumn;
pub use catalog_constraint::CatalogConstraint;
pub use catalog_index::CatalogIndex;
pub use introspector::SqlIntrospector;
pub(crate) use introspector::normalize_type;
pub use role_access::RoleAccess;
pub use table_catalog::TableCatalog;
