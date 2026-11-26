/// A constraint read from `pg_constraint`; `NOT NULL` is a column property instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogConstraint {
    /// The constraint's name.
    pub name: String,
    /// The table the constraint is declared on, another table's for a foreign key referencing this one.
    pub table: String,
    /// The engine's one-letter kind: `p` primary key, `u` unique, `c` check, `f` foreign
    /// key, `x` exclusion, `t` constraint trigger.
    pub kind: char,
    /// The definition as `pg_get_constraintdef` renders it.
    pub definition: String,
}
