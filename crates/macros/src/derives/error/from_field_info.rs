//! Collected `#[from]` field data for `From<T>` impl generation.

use syn::{Ident, Type};

pub(super) struct FromFieldInfo {
    pub variant_ident: Ident,
    pub from_type: Type,
    pub is_tuple: bool,
    pub field_name: Option<Ident>,
}
