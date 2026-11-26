/// Column definition for schema.
#[derive(Debug, Clone, PartialEq)]
pub struct TableColumn {
    /// Column name.
    pub name: String,
    /// SQL/CQL type.
    pub data_type: String,
    /// Whether nullable.
    pub nullable: bool,
    /// Default value expression.
    pub default: Option<String>,
    /// Whether this is part of primary key.
    pub primary_key: bool,
    /// Whether unique.
    pub unique: bool,
}
