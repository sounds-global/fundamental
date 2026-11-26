/// A column read from `pg_attribute`, in the vocabulary the `Model` derive writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogColumn {
    /// The column's name.
    pub name: String,
    /// Its type, normalised to the derive's name; an unknown type is kept upper-cased so it compares unequal.
    pub data_type: String,
    /// Whether the column admits `NULL`.
    pub nullable: bool,
    /// The column's `DEFAULT` expression, if it has one.
    pub default: Option<String>,
    /// The collation a collatable column compares with, or `None` for a type that does not collate.
    pub collation: Option<String>,
}
