/// A monotonic guard on one column deciding whether a conflicting upsert may overwrite the
/// stored row, comparing `EXCLUDED.<column>` with the stored value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColumnGuard {
    /// Overwrite only when `EXCLUDED.<column> >= <stored>.<column>`.
    ExcludedGreaterOrEqual(String),
    /// Overwrite only when `EXCLUDED.<column> > <stored>.<column>`.
    ExcludedGreater(String),
}

impl ColumnGuard {
    /// A guard that admits the write when `EXCLUDED.<column> >= <stored>.<column>`.
    #[must_use]
    pub fn excluded_ge(column: impl Into<String>) -> Self {
        Self::ExcludedGreaterOrEqual(column.into())
    }

    /// A guard that admits the write when `EXCLUDED.<column> > <stored>.<column>`.
    #[must_use]
    pub fn excluded_gt(column: impl Into<String>) -> Self {
        Self::ExcludedGreater(column.into())
    }

    pub(crate) fn column(&self) -> &str {
        match self {
            Self::ExcludedGreaterOrEqual(column) | Self::ExcludedGreater(column) => column,
        }
    }

    pub(crate) fn operator(&self) -> &'static str {
        match self {
            Self::ExcludedGreaterOrEqual(_) => ">=",
            Self::ExcludedGreater(_) => ">",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ge_and_gt_expose_their_column_and_operator() {
        let ge = ColumnGuard::excluded_ge("tree_size");
        assert_eq!(ge.column(), "tree_size");
        assert_eq!(ge.operator(), ">=");

        let gt = ColumnGuard::excluded_gt("epoch");
        assert_eq!(gt.column(), "epoch");
        assert_eq!(gt.operator(), ">");
    }
}
