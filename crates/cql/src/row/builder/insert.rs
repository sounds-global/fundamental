use crate::ToRow;

/// Helper to build INSERT CQL statements.
pub(crate) struct InsertBuilder {
    keyspace: String,
    table: String,
    columns: Vec<String>,
    ttl: Option<i32>,
    write_time: Option<i64>,
    if_not_exists: bool,
}

impl InsertBuilder {
    pub(crate) fn new(keyspace: impl Into<String>, table: impl Into<String>) -> Self {
        Self {
            keyspace: keyspace.into(),
            table: table.into(),
            columns: Vec::new(),
            ttl: None,
            write_time: None,
            if_not_exists: false,
        }
    }

    pub(crate) fn columns_from<T: ToRow>(mut self) -> Self {
        self.columns = T::columns().iter().map(|s| (*s).to_string()).collect();
        self
    }

    pub(crate) fn ttl(mut self, seconds: i32) -> Self {
        self.ttl = Some(seconds);
        self
    }

    /// Sets the cell write time (`USING TIMESTAMP`), in microseconds since the Unix epoch.
    pub(crate) fn write_time(mut self, micros: i64) -> Self {
        self.write_time = Some(micros);
        self
    }

    pub(crate) fn if_not_exists(mut self) -> Self {
        self.if_not_exists = true;
        self
    }

    pub(crate) fn build(&self) -> String {
        let placeholders: Vec<&str> = self.columns.iter().map(|_| "?").collect();

        let mut cql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            super::qualify(&self.keyspace, &self.table),
            self.columns.join(", "),
            placeholders.join(", ")
        );

        if self.if_not_exists {
            cql.push_str(" IF NOT EXISTS");
        }

        // `USING` takes both options in one clause joined by `AND`; emitting two
        // separate `USING` clauses is a syntax error.
        match (self.ttl, self.write_time) {
            (Some(ttl), Some(micros)) => {
                cql.push_str(&format!(" USING TTL {ttl} AND TIMESTAMP {micros}"));
            }
            (Some(ttl), None) => cql.push_str(&format!(" USING TTL {ttl}")),
            (None, Some(micros)) => cql.push_str(&format!(" USING TIMESTAMP {micros}")),
            (None, None) => {}
        }

        cql
    }
}
