use crate::ToRow;

/// Helper to build UPDATE CQL statements.
pub(crate) struct UpdateBuilder {
    keyspace: String,
    table: String,
    set_columns: Vec<String>,
    where_columns: Vec<String>,
    ttl: Option<i32>,
    write_time: Option<i64>,
    if_exists: bool,
}

impl UpdateBuilder {
    pub(crate) fn new(keyspace: impl Into<String>, table: impl Into<String>) -> Self {
        Self {
            keyspace: keyspace.into(),
            table: table.into(),
            set_columns: Vec::new(),
            where_columns: Vec::new(),
            ttl: None,
            write_time: None,
            if_exists: false,
        }
    }

    /// Sets the columns to update (value columns only).
    pub(crate) fn set_columns_from<T: ToRow>(mut self) -> Self {
        self.set_columns = T::value_columns()
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        self
    }

    pub(crate) fn where_columns_from<T: ToRow>(mut self) -> Self {
        let mut cols: Vec<String> = T::partition_key_columns()
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        cols.extend(T::clustering_key_columns().iter().map(|s| (*s).to_string()));
        self.where_columns = cols;
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

    pub(crate) fn if_exists(mut self) -> Self {
        self.if_exists = true;
        self
    }

    pub(crate) fn build(&self) -> String {
        let set_clause: Vec<String> = self
            .set_columns
            .iter()
            .map(|col| format!("{} = ?", col))
            .collect();

        let where_clause: Vec<String> = self
            .where_columns
            .iter()
            .map(|col| format!("{} = ?", col))
            .collect();

        let mut cql = String::new();

        cql.push_str("UPDATE ");
        cql.push_str(&super::qualify(&self.keyspace, &self.table));

        // `USING` takes both options in one clause joined by `AND`; two separate
        // `USING` clauses is a syntax error.
        match (self.ttl, self.write_time) {
            (Some(ttl), Some(micros)) => {
                cql.push_str(&format!(" USING TTL {ttl} AND TIMESTAMP {micros}"));
            }
            (Some(ttl), None) => cql.push_str(&format!(" USING TTL {ttl}")),
            (None, Some(micros)) => cql.push_str(&format!(" USING TIMESTAMP {micros}")),
            (None, None) => {}
        }

        cql.push_str(" SET ");
        cql.push_str(&set_clause.join(", "));
        cql.push_str(" WHERE ");
        cql.push_str(&where_clause.join(" AND "));

        if self.if_exists {
            cql.push_str(" IF EXISTS");
        }

        cql
    }
}
