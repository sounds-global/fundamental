//! [`SqlType`] — the YSQL type a Rust type is stored as.

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};

/// The YSQL type a value of this Rust type is stored as; an `Option` is stored as its inner type, nullable.
pub trait SqlType {
    /// The type, as a migration writes it.
    const SQL: &'static str;
}

macro_rules! stored_as {
    ($($rust:ty => $sql:literal),* $(,)?) => {
        $(impl SqlType for $rust {
            const SQL: &'static str = $sql;
        })*
    };
}

stored_as! {
    String => "TEXT",
    bool => "BOOL",
    i16 => "INT2",
    i32 => "INT4",
    u16 => "INT4",
    i8 => "INT8",
    i64 => "INT8",
    u8 => "INT8",
    u32 => "INT8",
    u64 => "INT8",
    f32 => "FLOAT4",
    f64 => "FLOAT8",
    uuid::Uuid => "UUID",
    Vec<u8> => "BYTEA",
    Vec<String> => "JSONB",
}

#[cfg(feature = "chrono")]
stored_as! {
    chrono::NaiveDate => "DATE",
    chrono::DateTime<chrono::Utc> => "TIMESTAMPTZ",
}

impl<T: SqlType> SqlType for Option<T> {
    const SQL: &'static str = T::SQL;
}

#[cfg(test)]
mod tests {
    use super::SqlType;

    #[test]
    fn an_option_is_stored_as_its_inner_type() {
        assert_eq!(<Option<uuid::Uuid> as SqlType>::SQL, "UUID");
        assert_eq!(<Option<String> as SqlType>::SQL, "TEXT");
    }
}
