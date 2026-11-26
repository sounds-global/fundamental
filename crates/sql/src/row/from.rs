//! Constructing model types from YSQL rows.

use sqlx::postgres::PgRow;

use fundamental_encryption::KeyManager;

use crate::SqlResult;

/// Types that can be constructed from a database row, usually via `#[derive(Model)]`.
///
/// # Examples
///
/// ```rust
/// use fundamental_sql::{SqlResult, FromRow};
/// // `PgRow` is sqlx's Postgres row; `Row` brings `try_get` into scope.
/// use sqlx::Row;
/// use sqlx::postgres::PgRow;
/// use uuid::Uuid;
///
/// struct User {
///     id: Uuid,
///     email: String,
///     name: String,
/// }
///
/// impl FromRow for User {
///     fn from_row(row: &PgRow) -> SqlResult<Self> {
///         Ok(Self {
///             id: row.try_get("id")?,
///             email: row.try_get("email")?,
///             name: row.try_get("name")?,
///         })
///     }
/// }
/// ```
pub trait FromRow: Sized {
    /// Constructs an instance from a database row.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    fn from_row(row: &PgRow) -> SqlResult<Self>;

    /// Constructs an instance from a database row, returning None if the row is null.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    fn from_row_optional(row: &PgRow) -> SqlResult<Option<Self>> {
        Self::from_row(row).map(Some)
    }

    /// Constructs an instance from a row, decrypting any `Secret` columns with `enc`.
    ///
    /// # Errors
    ///
    /// Returns an error if a `Secret` column needs decryption but `enc` is `None`,
    /// or if decryption fails.
    fn from_row_encrypted(row: &PgRow, enc: Option<&KeyManager>) -> SqlResult<Self> {
        let _ = enc;
        Self::from_row(row)
    }
}
