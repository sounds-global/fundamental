//! Reading Rust types back out of YCQL rows.

use fundamental_encryption::KeyManager;

use crate::CqlResult;
use scylla::value::{CqlValue, Row};

/// Reads a single CQL column value into a Rust type.
///
/// `#[derive(Identifier)]` implements it for a newtype id by delegating to the inner type.
pub trait FromCqlValue: Sized {
    /// Converts a `CqlValue` into `Self`, or `None` on a type/null mismatch.
    fn from_cql_value(value: &CqlValue) -> Option<Self>;
}

impl FromCqlValue for String {
    fn from_cql_value(value: &CqlValue) -> Option<Self> {
        value.as_text().cloned()
    }
}
impl FromCqlValue for i64 {
    fn from_cql_value(value: &CqlValue) -> Option<Self> {
        value.as_bigint()
    }
}
impl FromCqlValue for i32 {
    fn from_cql_value(value: &CqlValue) -> Option<Self> {
        value.as_int()
    }
}
impl FromCqlValue for i16 {
    fn from_cql_value(value: &CqlValue) -> Option<Self> {
        value.as_smallint()
    }
}
impl FromCqlValue for i8 {
    fn from_cql_value(value: &CqlValue) -> Option<Self> {
        value.as_tinyint()
    }
}
impl FromCqlValue for f64 {
    fn from_cql_value(value: &CqlValue) -> Option<Self> {
        value.as_double()
    }
}
impl FromCqlValue for f32 {
    fn from_cql_value(value: &CqlValue) -> Option<Self> {
        value.as_float()
    }
}
impl FromCqlValue for bool {
    fn from_cql_value(value: &CqlValue) -> Option<Self> {
        value.as_boolean()
    }
}
impl FromCqlValue for uuid::Uuid {
    fn from_cql_value(value: &CqlValue) -> Option<Self> {
        value.as_uuid()
    }
}
impl FromCqlValue for Vec<u8> {
    fn from_cql_value(value: &CqlValue) -> Option<Self> {
        value.as_blob().cloned()
    }
}

/// Trait for types that can be constructed from a YCQL row.
///
/// # Example
///
/// ```rust
/// use fundamental_cql::{FromRow, CqlError, CqlResult};
/// use scylla::value::{CqlValue, Row};
/// use uuid::Uuid;
///
/// struct Event {
///     user_id: Uuid,
///     timestamp: i64,
///     event_type: String,
///     data: Vec<u8>,
/// }
///
/// impl FromRow for Event {
///     fn from_row(row: Row) -> CqlResult<Self> {
///         // Columns arrive in `SELECT` order as `Vec<Option<CqlValue>>`.
///         let mut cols = row.columns.into_iter();
///         let decode = || CqlError::RowConversion("missing or wrong column".into());
///         let user_id = cols.next().flatten().and_then(|v| v.as_uuid()).ok_or_else(decode)?;
///         let timestamp = cols.next().flatten().and_then(|v| v.as_bigint()).ok_or_else(decode)?;
///         let event_type = cols.next().flatten().and_then(|v| v.into_string()).ok_or_else(decode)?;
///         let data = cols.next().flatten().and_then(|v| v.into_blob()).ok_or_else(decode)?;
///         Ok(Self { user_id, timestamp, event_type, data })
///     }
/// }
/// ```
pub trait FromRow: Sized {
    /// Constructs an instance from a YCQL row.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    fn from_row(row: Row) -> CqlResult<Self>;

    /// Constructs an instance from a row, decrypting any
    /// [`secret_columns`](fundamental_core::Model::secret_columns) with `enc`.
    ///
    /// # Errors
    ///
    /// Returns an error if a `Secret` column needs decryption but `enc` is `None`,
    /// or if decryption fails.
    fn from_row_encrypted(row: Row, enc: Option<&KeyManager>) -> CqlResult<Self> {
        let _ = enc;
        Self::from_row(row)
    }
}
