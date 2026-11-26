//! [`KeyDeclaration`]: a whole primary-key or index-key list, as its author wrote it.

use core::fmt;

use super::KeyElement;

/// The elements of one key, in declaration order.
///
/// At most one leading element may be `HASH`, every later one is `ASC` or `DESC`, and every
/// element may be undeclared, which leaves the engine to choose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyDeclaration {
    elements: Vec<KeyElement>,
}

impl KeyDeclaration {
    /// Builds a declaration from `elements`, in declaration order.
    #[must_use]
    pub fn new(elements: Vec<KeyElement>) -> Self {
        Self { elements }
    }

    /// The elements, in declaration order.
    #[must_use]
    pub fn elements(&self) -> &[KeyElement] {
        &self.elements
    }

    /// The elements that named a column and nothing else.
    #[must_use]
    pub fn undeclared(&self) -> Vec<&KeyElement> {
        self.elements.iter().filter(|e| e.is_undeclared()).collect()
    }

    /// The columns of the leading hash component, or empty when there is none.
    ///
    /// A read must supply all of these to reach one tablet: the hash is over the tuple.
    #[must_use]
    pub fn hash_columns(&self) -> &[String] {
        match self.elements.first() {
            Some(first) if first.is_hash() => first.columns(),
            _ => &[],
        }
    }

    /// Every column the key names, hash component and range columns alike.
    #[must_use]
    pub fn columns(&self) -> Vec<&str> {
        self.elements
            .iter()
            .flat_map(|e| e.columns().iter().map(String::as_str))
            .collect()
    }
}

impl fmt::Display for KeyDeclaration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rendered: Vec<String> = self.elements.iter().map(ToString::to_string).collect();
        write!(f, "({})", rendered.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::KeyDeclaration;
    use crate::migration::ddl::{KeyElement, Sharding};

    fn zone_grant_seq() -> KeyDeclaration {
        KeyDeclaration::new(vec![
            KeyElement::new(
                vec!["zone".to_owned(), "grant_id".to_owned()],
                Some(Sharding::Hash),
            ),
            KeyElement::new(vec!["seq".to_owned()], Some(Sharding::Asc)),
        ])
    }

    #[test]
    fn the_hash_component_is_the_whole_leading_group() {
        let key = zone_grant_seq();

        assert_eq!(key.hash_columns(), ["zone", "grant_id"]);
        assert!(key.undeclared().is_empty());
    }

    #[test]
    fn a_range_only_key_has_no_hash_component() {
        let ordered = KeyDeclaration::new(vec![KeyElement::new(
            vec!["leaf_index".to_owned()],
            Some(Sharding::Asc),
        )]);

        assert!(ordered.hash_columns().is_empty());
        assert_eq!(ordered.columns(), ["leaf_index"]);
    }

    #[test]
    fn an_undeclared_leading_element_is_not_reported_as_hashed() {
        let key = KeyDeclaration::new(vec![
            KeyElement::new(vec!["zone".to_owned()], None),
            KeyElement::new(vec!["id".to_owned()], None),
        ]);

        assert!(
            key.hash_columns().is_empty(),
            "the engine will hash `zone`, but the migration did not say so — and the \
             difference is the whole point"
        );
        assert_eq!(key.undeclared().len(), 2);
    }

    #[test]
    fn it_renders_the_way_the_engine_reads_it_back() {
        assert_eq!(
            zone_grant_seq().to_string(),
            "((zone, grant_id) HASH, seq ASC)"
        );
    }
}
