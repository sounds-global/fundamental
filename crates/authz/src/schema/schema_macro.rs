//! The [`authz_schema!`](crate::authz_schema) macro.

/// Assembles a compiled [`SchemaIr`](crate::schema::SchemaIr) from `#[derive(Namespace)]`
/// types: `SchemaIr::builder().namespace::<User>()…build()`.
///
/// ```
/// use fundamental_authz::authz_schema;
/// use fundamental_authz::schema::SchemaIr;
/// use fundamental_macros::Namespace;
///
/// #[derive(Namespace)]
/// enum User {}
///
/// #[derive(Namespace)]
/// enum Document {
///     #[relation(direct(User))]
///     Owner,
/// }
///
/// let schema: SchemaIr = authz_schema![User, Document].expect("valid schema");
/// assert_eq!(schema.len(), 2);
/// ```
///
/// A shared, build-once schema:
///
/// ```
/// # extern crate fundamental_authz;
/// # use std::sync::{Arc, LazyLock};
/// # use fundamental_authz::authz_schema;
/// # use fundamental_authz::schema::SchemaIr;
/// # use fundamental_macros::Namespace;
/// # #[derive(Namespace)] enum User {}
/// # #[derive(Namespace)] enum Document { #[relation(direct(User))] Owner }
/// static SCHEMA: LazyLock<Arc<SchemaIr>> =
///     LazyLock::new(|| Arc::new(authz_schema![User, Document].expect("valid schema")));
///
/// assert_eq!(SCHEMA.len(), 2);
/// ```
#[macro_export]
macro_rules! authz_schema {
    ($($namespace:ty),+ $(,)?) => {
        $crate::schema::SchemaIr::builder()
            $(.namespace::<$namespace>())+
            .build()
    };
}
