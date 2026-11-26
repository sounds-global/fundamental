//! The [`Ddl`] schema statement.

use std::borrow::Cow;

/// A CQL schema statement — `CREATE`, `ALTER`, `DROP`.
///
/// Separate from [`Cql`](crate::Cql): a schema statement cannot be prepared and binds nothing.
///
/// # Example
///
/// ```rust
/// # extern crate fundamental_cql;
/// use fundamental_cql::{CqlIdent, Ddl, ddl};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let table = CqlIdent::qualified("layer2", "grants")?;
/// let statement = ddl!("DROP TABLE IF EXISTS {}", table);
///
/// assert_eq!(statement.text(), "DROP TABLE IF EXISTS layer2.grants");
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct Ddl {
    text: Cow<'static, str>,
}

impl Ddl {
    /// A schema statement whose text is known at compile time.
    #[must_use]
    pub fn new(text: &'static str) -> Self {
        Self {
            text: Cow::Borrowed(text),
        }
    }

    /// A schema statement whose text was built at runtime — the escape hatch.
    ///
    /// Reach for [`ddl!`](crate::ddl) first; this is for text such as a migration file's contents.
    #[must_use]
    pub fn interpolated(text: String) -> Self {
        Self {
            text: Cow::Owned(text),
        }
    }

    /// The statement text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Builds a [`Ddl`] statement, interpolating only [`CqlIdent`](crate::CqlIdent)s.
///
/// The twin of [`cql!`](crate::cql), for schema statements. The format string
/// must be a literal and every argument must be a `CqlIdent`.
///
/// # Example
///
/// ```rust
/// # extern crate fundamental_cql;
/// use fundamental_cql::{CqlIdent, ddl};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let keyspace = CqlIdent::new("suite_vault")?;
/// let statement = ddl!("DROP KEYSPACE IF EXISTS {}", keyspace);
///
/// assert_eq!(statement.text(), "DROP KEYSPACE IF EXISTS suite_vault");
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! ddl {
    ($text:literal) => {
        $crate::Ddl::new($text)
    };
    ($text:literal, $($ident:expr),+ $(,)?) => {
        $crate::Ddl::interpolated(::std::format!(
            $text,
            $($crate::CqlIdent::as_cql(&$ident)),+
        ))
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CqlIdent;

    #[test]
    fn a_static_schema_statement_keeps_its_text() {
        let statement = Ddl::new("CREATE TABLE IF NOT EXISTS t (id text PRIMARY KEY)");

        assert_eq!(
            statement.text(),
            "CREATE TABLE IF NOT EXISTS t (id text PRIMARY KEY)"
        );
    }

    #[test]
    fn the_macro_interpolates_a_validated_identifier() {
        let ledger = CqlIdent::qualified("layer0", "_fundamental_migrations").expect("valid");

        let statement = ddl!(
            "CREATE TABLE IF NOT EXISTS {} (name text PRIMARY KEY)",
            ledger
        );

        assert_eq!(
            statement.text(),
            "CREATE TABLE IF NOT EXISTS layer0.\"_fundamental_migrations\" \
             (name text PRIMARY KEY)"
        );
    }
}
