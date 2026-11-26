//! [`KeyElement`]: one component of a primary key or index key, as written.

use core::fmt;

use super::Sharding;

/// One element of a key, carrying the sharding its author declared, or none.
///
/// Several columns only for a composite hash component, `((zone, grant_id) HASH, …)`.
/// `sharding` is [`None`] when the author declared nothing, which is legal; refusing it is
/// [`MigrationGuard`](crate::migration::MigrationGuard)'s job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyElement {
    columns: Vec<String>,
    sharding: Option<Sharding>,
    nulls: Option<crate::migration::Nulls>,
}

impl KeyElement {
    /// Builds an element over `columns`, distributed by `sharding` when one was declared.
    ///
    /// # Panics
    /// Panics if `columns` is empty, which the recogniser never produces.
    #[must_use]
    pub fn new(columns: Vec<String>, sharding: Option<Sharding>) -> Self {
        assert!(
            !columns.is_empty(),
            "a key element names at least one column"
        );
        Self {
            columns,
            sharding,
            nulls: None,
        }
    }

    /// The columns this element covers, in declaration order.
    #[must_use]
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// It keeping its empty values where `nulls` says.
    #[must_use]
    pub const fn with_nulls(mut self, nulls: Option<crate::migration::Nulls>) -> Self {
        self.nulls = nulls;
        self
    }

    /// Where it keeps its empty values, when not where the engine keeps them by default.
    #[must_use]
    pub const fn nulls(&self) -> Option<crate::migration::Nulls> {
        self.nulls
    }

    /// How the element's values are distributed, when the author said.
    #[must_use]
    pub fn sharding(&self) -> Option<Sharding> {
        self.sharding
    }

    /// Whether this element left its distribution to the engine.
    #[must_use]
    pub fn is_undeclared(&self) -> bool {
        self.sharding.is_none()
    }

    /// Whether this element distributes by hash.
    #[must_use]
    pub fn is_hash(&self) -> bool {
        self.sharding.is_some_and(Sharding::is_hash)
    }
}

impl fmt::Display for KeyElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let columns = if self.columns.len() == 1 {
            self.columns[0].clone()
        } else {
            format!("({})", self.columns.join(", "))
        };
        match self.sharding {
            Some(sharding) => write!(f, "{columns} {sharding}"),
            None => f.write_str(&columns),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::KeyElement;
    use crate::migration::ddl::Sharding;

    #[test]
    fn a_single_column_element_renders_as_the_engine_reads_it_back() {
        let element = KeyElement::new(vec!["leaf_index".to_owned()], Some(Sharding::Asc));

        assert_eq!(element.to_string(), "leaf_index ASC");
        assert!(!element.is_undeclared());
        assert!(!element.is_hash());
    }

    #[test]
    fn a_composite_hash_component_keeps_its_parentheses() {
        let element = KeyElement::new(
            vec!["zone".to_owned(), "grant_id".to_owned()],
            Some(Sharding::Hash),
        );

        assert_eq!(element.to_string(), "(zone, grant_id) HASH");
        assert_eq!(element.columns().len(), 2);
        assert!(element.is_hash());
    }

    #[test]
    fn an_undeclared_element_renders_as_the_author_left_it() {
        let element = KeyElement::new(vec!["zone".to_owned()], None);

        assert_eq!(element.to_string(), "zone");
        assert!(element.is_undeclared());
        assert!(
            !element.is_hash(),
            "undeclared is not the same as not hashed"
        );
    }
}
