//! [`DdlColumn`]: a column a `CREATE TABLE` declares, as the lints read it.

/// A column as a `CREATE TABLE` writes it: its name, its type as written, and its default.
///
/// An identity column has the default `identity`, read as `nextval(…)` is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdlColumn {
    /// The column's name.
    pub name: String,
    /// Its type, uppercased as written: `TIMESTAMPTZ`, `NUMERIC(10, 2)`.
    pub data_type: String,
    /// Its default as written, when it has one.
    pub default: Option<String>,
}

impl DdlColumn {
    /// Whether every new row takes a value above the last: a time of writing or a sequence.
    #[must_use]
    pub fn only_increases(&self) -> bool {
        let data_type = self.data_type.as_str();
        let default = self
            .default
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        matches!(data_type, "TIMESTAMPTZ" | "TIMESTAMP" | "DATE")
            || data_type.starts_with("TIMESTAMP")
            || matches!(data_type, "SERIAL" | "BIGSERIAL" | "SMALLSERIAL")
            || default.starts_with("now()")
            || default.contains("current_timestamp")
            || default.starts_with("nextval(")
            || default == "identity"
    }
}

#[cfg(test)]
mod tests {
    use super::DdlColumn;

    fn column(data_type: &str, default: Option<&str>) -> DdlColumn {
        DdlColumn {
            name: "at".to_owned(),
            data_type: data_type.to_owned(),
            default: default.map(str::to_owned),
        }
    }

    #[test]
    fn a_time_or_a_sequence_only_increases() {
        for increasing in [
            column("TIMESTAMPTZ", None),
            column("DATE", None),
            column("BIGSERIAL", None),
            column("INT8", Some("nextval('orders_seq')")),
            column("INT8", Some("identity")),
            column("TEXT", Some("now()::text")),
        ] {
            assert!(increasing.only_increases(), "{increasing:?}");
        }
    }

    #[test]
    fn an_id_or_a_name_does_not() {
        for spread in [
            column("UUID", Some("gen_random_uuid()")),
            column("TEXT", None),
        ] {
            assert!(!spread.only_increases(), "{spread:?}");
        }
    }
}
