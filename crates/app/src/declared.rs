use std::collections::BTreeMap;

use fundamental_core::ServiceDeclaration;

/// An app that declares nothing beyond its name.
struct Named(String);

impl ServiceDeclaration for Named {
    fn service(&self) -> &str {
        &self.0
    }

    fn rendered(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }
}

/// The declaration an app's [`App::admin`](crate::App::admin) builds, its type erased.
pub(crate) struct Declared(Box<dyn ServiceDeclaration>);

impl Declared {
    pub(crate) fn new(declaration: Box<dyn ServiceDeclaration>) -> Self {
        Self(declaration)
    }

    /// The declaration of an app that declares nothing beyond its `name`.
    pub(crate) fn named(name: &str) -> Self {
        Self(Box::new(Named(name.to_owned())))
    }
}

impl ServiceDeclaration for Declared {
    fn service(&self) -> &str {
        self.0.service()
    }

    fn rendered(&self) -> BTreeMap<String, String> {
        self.0.rendered()
    }
}
