use fundamental_db::migration::ddl::KeyDeclaration;

/// An index read from `pg_index`, with its key parsed by the migration guard's reader and its flags read directly.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(clippy::struct_excessive_bools, reason = "readability baseline")]
pub struct CatalogIndex {
    /// The index's name.
    pub name: String,
    /// The key list parsed from `pg_get_indexdef`, or `None` when it does not read as one.
    pub key: Option<KeyDeclaration>,
    /// The definition exactly as the engine renders it, for a message a person reads.
    pub definition: String,
    /// Whether this is the table's primary key.
    pub primary: bool,
    /// Whether the index refuses duplicate keys.
    pub unique: bool,
    /// Whether the index covers only rows matching a `WHERE` predicate.
    pub partial: bool,
    /// That predicate, as the engine renders it, without its outer parentheses.
    pub condition: Option<String>,
    /// Whether any key element is an expression rather than a column.
    pub expressions: bool,
    /// Whether the index carries non-key `INCLUDE` columns.
    pub includes: bool,
    /// The `INCLUDE` columns it carries beside its key, in order.
    pub include: Vec<String>,
    /// The access method — `lsm` for every index YugabyteDB distributes.
    pub method: String,
    /// Whether the index is valid, ready and live; an unfinished build keeps its name.
    pub usable: bool,
}
