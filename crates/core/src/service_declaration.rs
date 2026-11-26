//! What a process declares about itself in order to serve ([`ServiceDeclaration`]).

#[cfg(not(feature = "std"))]
use alloc::collections::BTreeMap;
#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(feature = "std")]
use std::collections::BTreeMap;

/// The operational facts a process states about itself, required to build its admin surface.
///
/// There is deliberately no blanket or `()` implementation: the argument cannot be satisfied by nothing.
/// Its rendering is public, so it never names a credential or request data.
pub trait ServiceDeclaration: Send + Sync + 'static {
    /// The declaring service's name, as logs and the readiness payload report it.
    fn service(&self) -> &str;

    /// The declaration, flattened for publication.
    ///
    /// A sorted map, so two reads of one unchanged process are identical.
    fn rendered(&self) -> BTreeMap<String, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Declaration;

    impl ServiceDeclaration for Declaration {
        fn service(&self) -> &str {
            "layer0"
        }

        fn rendered(&self) -> BTreeMap<String, String> {
            BTreeMap::from([
                ("replicas".to_string(), "3".to_string()),
                ("pool.sql".to_string(), "10".to_string()),
            ])
        }
    }

    #[test]
    fn a_declaration_names_itself_and_renders() {
        let declaration = Declaration;
        assert_eq!(declaration.service(), "layer0");
        assert_eq!(
            declaration.rendered().get("replicas").map(String::as_str),
            Some("3")
        );
    }

    #[test]
    fn rendering_is_stable_between_reads() {
        let declaration = Declaration;
        let keys: Vec<String> = declaration.rendered().into_keys().collect();
        assert_eq!(keys, vec!["pool.sql", "replicas"]);
        assert_eq!(declaration.rendered(), declaration.rendered());
    }

    #[test]
    fn a_declaration_is_object_safe() {
        let declaration: Box<dyn ServiceDeclaration> = Box::new(Declaration);
        assert_eq!(declaration.service(), "layer0");
    }
}
