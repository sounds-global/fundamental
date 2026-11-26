//! [`Relation`]: the trait a namespace's relation enum implements.

/// A relation on a namespace, as a typed value.
pub trait Relation {
    /// The relation's name, a safe identifier.
    fn relation_name(&self) -> &'static str;
}
