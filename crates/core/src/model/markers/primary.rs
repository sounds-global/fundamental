//! The [`Primary`] marker: the built-in default database connection.

use super::Connection;

/// The default database connection, for a model that names no `#[model(connection = …)]`.
///
/// ```rust
/// # extern crate fundamental_core as fundamental;
/// use fundamental::model::{Connection, Primary};
///
/// assert_eq!(Primary::NAME, "default");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Primary;

impl Connection for Primary {
    const NAME: &'static str = "default";
}

#[cfg(test)]
mod tests {
    use super::{Connection, Primary};

    #[test]
    fn primary_routes_to_the_default_domain() {
        assert_eq!(Primary::NAME, "default");
    }
}
