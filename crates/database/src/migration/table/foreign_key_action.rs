/// Foreign key action type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignKeyAction {
    /// No action.
    NoAction,
    /// Restrict.
    Restrict,
    /// Cascade.
    Cascade,
    /// Set null.
    SetNull,
    /// Set default.
    SetDefault,
}

impl ForeignKeyAction {
    /// Returns the SQL representation.
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::NoAction => "NO ACTION",
            Self::Restrict => "RESTRICT",
            Self::Cascade => "CASCADE",
            Self::SetNull => "SET NULL",
            Self::SetDefault => "SET DEFAULT",
        }
    }
}
