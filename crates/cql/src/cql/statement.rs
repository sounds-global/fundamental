//! The [`Cql`] data statement.

use std::borrow::Cow;

use crate::{Consistency, CqlValue, CqlValues};

/// A CQL data statement — its text, its bound values, and the per-statement
/// options the driver accepts.
///
/// [`new`](Self::new) takes `&'static str`, names are spliced only by [`cql!`](crate::cql), and
/// values are always bound.
///
/// # Example
///
/// ```rust
/// # extern crate fundamental_cql;
/// use fundamental_cql::{Consistency, Cql, CqlValue};
///
/// let statement = Cql::new("SELECT payload FROM events WHERE tenant = ?")
///     .bind(CqlValue::text("acme"))
///     .consistency(Consistency::LocalQuorum);
///
/// assert_eq!(statement.values().0.len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct Cql {
    text: Cow<'static, str>,
    values: CqlValues,
    consistency: Option<Consistency>,
    page_size: Option<i32>,
    idempotent: bool,
}

impl Cql {
    /// A statement whose text is known at compile time.
    #[must_use]
    pub fn new(text: &'static str) -> Self {
        Self::of(Cow::Borrowed(text))
    }

    /// A statement whose text was built at runtime — the escape hatch.
    ///
    /// Reach for [`cql!`](crate::cql) first; this is for text such as migration CQL read from a
    /// file.
    #[must_use]
    pub fn interpolated(text: String) -> Self {
        Self::of(Cow::Owned(text))
    }

    fn of(text: Cow<'static, str>) -> Self {
        Self {
            text,
            values: CqlValues(Vec::new()),
            consistency: None,
            page_size: None,
            idempotent: false,
        }
    }

    /// Appends one bound value, in placeholder order.
    #[must_use]
    pub fn bind(mut self, value: CqlValue) -> Self {
        self.values.0.push(value);
        self
    }

    /// Replaces the bound values, in placeholder order.
    #[must_use]
    pub fn values_of(mut self, values: Vec<CqlValue>) -> Self {
        self.values = CqlValues(values);
        self
    }

    /// Overrides the consistency the executor would otherwise take from the
    /// pool's configured read or write level.
    #[must_use]
    pub fn consistency(mut self, consistency: Consistency) -> Self {
        self.consistency = Some(consistency);
        self
    }

    /// Sets the page size used by the paged read path.
    #[must_use]
    pub fn page_size(mut self, rows: i32) -> Self {
        self.page_size = Some(rows);
        self
    }

    /// Marks the statement as safe to retry on another node.
    ///
    /// Only true when re-running it cannot change the outcome — a `SELECT`, or
    /// an idempotent upsert. A counter update is never idempotent.
    #[must_use]
    pub fn idempotent(mut self) -> Self {
        self.idempotent = true;
        self
    }

    /// The statement text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The bound values, in placeholder order.
    #[must_use]
    pub fn values(&self) -> &CqlValues {
        &self.values
    }

    /// The consistency override, if one was set.
    #[must_use]
    pub fn consistency_override(&self) -> Option<Consistency> {
        self.consistency
    }

    /// The page size override, if one was set.
    #[must_use]
    pub fn page_size_override(&self) -> Option<i32> {
        self.page_size
    }

    /// Whether the statement was declared safe to retry.
    #[must_use]
    pub fn is_idempotent(&self) -> bool {
        self.idempotent
    }
}

/// Builds a [`Cql`] statement, interpolating only [`CqlIdent`](crate::CqlIdent)s.
///
/// The format string must be a literal and every argument a `CqlIdent`; a `String` will not
/// compile.
///
/// # Example
///
/// ```rust
/// # extern crate fundamental_cql;
/// use fundamental_cql::{CqlIdent, cql};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let ledger = CqlIdent::qualified("layer0", "_fundamental_migrations")?;
/// let statement = cql!("SELECT name FROM {} WHERE name = ?", ledger);
///
/// assert!(statement.text().starts_with("SELECT name FROM layer0."));
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! cql {
    ($text:literal) => {
        $crate::Cql::new($text)
    };
    ($text:literal, $($ident:expr),+ $(,)?) => {
        $crate::Cql::interpolated(::std::format!(
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
    fn a_static_statement_carries_no_values_by_default() {
        let statement = Cql::new("SELECT now() FROM system.local");

        assert_eq!(statement.text(), "SELECT now() FROM system.local");
        assert!(statement.values().0.is_empty());
        assert_eq!(statement.consistency_override(), None);
        assert!(!statement.is_idempotent());
    }

    #[test]
    fn values_bind_in_placeholder_order() {
        let statement = Cql::new("SELECT a FROM t WHERE b = ? AND c = ?")
            .bind(CqlValue::text("first"))
            .bind(CqlValue::Int(2));

        assert!(matches!(statement.values().0.as_slice(), [
            CqlValue::Text(b),
            CqlValue::Int(2)
        ] if b == "first"));
    }

    #[test]
    fn the_options_survive_the_builder() {
        let statement = Cql::new("SELECT a FROM t")
            .consistency(Consistency::EachQuorum)
            .page_size(250)
            .idempotent();

        assert_eq!(
            statement.consistency_override(),
            Some(Consistency::EachQuorum)
        );
        assert_eq!(statement.page_size_override(), Some(250));
        assert!(statement.is_idempotent());
    }

    #[test]
    fn the_macro_interpolates_a_validated_identifier() {
        let table = CqlIdent::qualified("layer2", "grants").expect("valid");

        let statement = cql!("SELECT id FROM {} WHERE id = ?", table);

        assert_eq!(
            statement.text(),
            "SELECT id FROM layer2.grants WHERE id = ?"
        );
    }

    #[test]
    fn the_macro_without_arguments_is_a_static_statement() {
        let statement = cql!("SELECT release_version FROM system.local");

        assert_eq!(statement.text(), "SELECT release_version FROM system.local");
    }
}
