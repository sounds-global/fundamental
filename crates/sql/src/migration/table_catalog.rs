use std::collections::{BTreeMap, BTreeSet};

use fundamental_db::migration::ddl::{DdlStatement, recognise};
use sqlx::Row as _;
use sqlx::postgres::types::Oid;
use sqlx::postgres::{PgPool, PgRow};
use tracing::{debug, instrument, warn};

use super::{CatalogColumn, CatalogConstraint, CatalogIndex, RoleAccess, normalize_type};
use crate::SqlError;
use crate::pool::SqlPool;

/// Everything about one table that changes what a statement against it does, read from
/// `pg_catalog` by the oid `to_regclass` resolves through the search path.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(clippy::struct_excessive_bools, reason = "readability baseline")]
pub struct TableCatalog {
    /// The table's name, as asked for.
    pub table: String,
    /// The columns, in declaration order.
    pub columns: Vec<CatalogColumn>,
    /// Every index, the primary key's included, ordered by name.
    pub indexes: Vec<CatalogIndex>,
    /// Every constraint on the table, and every foreign key elsewhere that references it, by name.
    pub constraints: Vec<CatalogConstraint>,
    /// User triggers; the engine's internal ones each enforce a listed constraint.
    pub triggers: Vec<String>,
    /// Rewrite rules.
    pub rules: Vec<String>,
    /// Whether row-level security is enabled.
    pub row_security: bool,
    /// Whether row-level security also binds the table's owner.
    pub row_security_forced: bool,
    /// Row-security policies.
    pub policies: Vec<String>,
    /// The roles a restrictive `SELECT` policy naming them alone narrows, with the rows each sees; a model's `read_by`.
    pub readers: BTreeMap<String, String>,
    /// Whether the table is partitioned, or is itself a partition.
    pub partitioned: bool,
    /// The tables it inherits from and the tables that inherit from it.
    pub inheritance: Vec<String>,
    /// Whether the table shares its tablets with other tables, colocated or in a tablegroup.
    pub colocated: bool,
    /// The tablets the table is split into, where the engine reports them.
    pub tablets: Option<u32>,
    /// The roles other than its owner and the connected role that may write its rows; a model's `written_by`.
    pub writers: BTreeSet<String>,
    /// What the connected role may do to the table's shape.
    pub access: RoleAccess,
}

impl TableCatalog {
    /// Reads `table` as the connection's search path resolves it, or `None` when no such table is on it.
    ///
    /// # Errors
    ///
    /// [`SqlError`] when a catalog read fails, or a row does not carry the column it was
    /// asked for.
    #[instrument(skip(pool), err(Debug))]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    pub async fn read(pool: &SqlPool, table: &str) -> Result<Option<Self>, SqlError> {
        let pool = pool.inner();
        let found = sqlx::query(
            "SELECT c.oid, c.relrowsecurity, c.relforcerowsecurity, \
                    c.relkind = 'p' OR c.relispartition AS partitioned, \
                    CASE WHEN c.relkind = 'r' THEN \
                        (SELECT p.is_colocated OR p.tablegroup_oid IS NOT NULL \
                         FROM yb_table_properties(c.oid) p) \
                    ELSE false END AS colocated, \
                    CASE WHEN c.relkind = 'r' THEN \
                        (SELECT p.num_tablets::int8 FROM yb_table_properties(c.oid) p) \
                    END AS tablets \
             FROM pg_class c WHERE c.oid = to_regclass($1) AND c.relkind IN ('r', 'p')",
        )
        .bind(table)
        .fetch_optional(&pool)
        .await?;
        let Some(found) = found else {
            debug!(table, "no such table on the search path");
            return Ok(None);
        };
        let oid: Oid = get(&found, "oid")?;
        let catalog = Self {
            table: table.to_owned(),
            columns: columns(&pool, oid, table).await?,
            indexes: indexes(&pool, oid, table).await?,
            constraints: constraints(&pool, oid).await?,
            triggers: names(
                &pool,
                oid,
                "SELECT tgname::text AS name FROM pg_trigger \
                 WHERE tgrelid = $1 AND NOT tgisinternal ORDER BY 1",
            )
            .await?,
            rules: names(
                &pool,
                oid,
                "SELECT rulename::text AS name FROM pg_rewrite WHERE ev_class = $1 ORDER BY 1",
            )
            .await?,
            row_security: get(&found, "relrowsecurity")?,
            row_security_forced: get(&found, "relforcerowsecurity")?,
            policies: names(
                &pool,
                oid,
                "SELECT polname::text AS name FROM pg_policy WHERE polrelid = $1 ORDER BY 1",
            )
            .await?,
            readers: if get(&found, "relrowsecurity")? {
                readers(&pool, oid).await?
            } else {
                BTreeMap::new()
            },
            partitioned: get(&found, "partitioned")?,
            inheritance: names(
                &pool,
                oid,
                "SELECT (CASE WHEN inhrelid = $1 THEN inhparent ELSE inhrelid END)::regclass::text \
                        AS name \
                 FROM pg_inherits WHERE inhrelid = $1 OR inhparent = $1 ORDER BY 1",
            )
            .await?,
            colocated: get::<Option<bool>>(&found, "colocated")?.unwrap_or(false),
            tablets: get::<Option<i64>>(&found, "tablets")?.and_then(|n| u32::try_from(n).ok()),
            writers: names(
                &pool,
                oid,
                "SELECT DISTINCT r.rolname::text AS name \
                 FROM pg_class c CROSS JOIN LATERAL aclexplode(c.relacl) a \
                 JOIN pg_roles r ON r.oid = a.grantee \
                 WHERE c.oid = $1 AND a.privilege_type IN ('INSERT', 'UPDATE', 'DELETE') \
                   AND a.grantee <> c.relowner AND r.rolname <> current_user \
                 ORDER BY 1",
            )
            .await?
            .into_iter()
            .collect(),
            access: access(&pool, oid).await?,
        };
        debug!(
            table,
            columns = catalog.columns.len(),
            indexes = catalog.indexes.len(),
            "read table catalog"
        );
        Ok(Some(catalog))
    }

    /// The index named `name`, if the table has one.
    #[must_use]
    pub fn index(&self, name: &str) -> Option<&CatalogIndex> {
        self.indexes.iter().find(|index| index.name == name)
    }

    /// The table's primary key, if it has one.
    #[must_use]
    pub fn primary_key(&self) -> Option<&CatalogIndex> {
        self.indexes.iter().find(|index| index.primary)
    }

    /// The column named `name`, if the table has one.
    #[must_use]
    pub fn column(&self, name: &str) -> Option<&CatalogColumn> {
        self.columns.iter().find(|column| column.name == name)
    }
}

async fn columns(pool: &PgPool, oid: Oid, table: &str) -> Result<Vec<CatalogColumn>, SqlError> {
    let rows = sqlx::query(
        "SELECT a.attname::text AS name, format_type(a.atttypid, a.atttypmod) AS data_type, \
                NOT a.attnotnull AS nullable, pg_get_expr(d.adbin, d.adrelid) AS default_expr, \
                co.collname::text AS collation \
         FROM pg_attribute a \
         LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
         LEFT JOIN pg_collation co ON co.oid = a.attcollation \
         WHERE a.attrelid = $1 AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY a.attnum",
    )
    .bind(oid)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            let name: String = get(row, "name")?;
            let raw_type: String = get(row, "data_type")?;
            Ok(CatalogColumn {
                data_type: normalize_type(&raw_type, table, &name),
                nullable: get(row, "nullable")?,
                default: get(row, "default_expr")?,
                collation: get(row, "collation")?,
                name,
            })
        })
        .collect()
}

#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn indexes(pool: &PgPool, oid: Oid, table: &str) -> Result<Vec<CatalogIndex>, SqlError> {
    let rows = sqlx::query(
        "SELECT c.relname::text AS name, pg_get_indexdef(i.indexrelid) AS definition, \
                i.indisprimary AS is_primary, i.indisunique AS is_unique, \
                i.indpred IS NOT NULL AS partial, \
                pg_get_expr(i.indpred, i.indrelid) AS condition, i.indexprs IS NOT NULL AS expressions, \
                i.indnkeyatts <> i.indnatts AS includes, \
                ARRAY(SELECT a.attname::text \
                      FROM generate_series(i.indnkeyatts, i.indnatts - 1) AS k \
                      JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = i.indkey[k] \
                      ORDER BY k) AS include, \
                am.amname::text AS method, \
                i.indisvalid AND i.indisready AND i.indislive AS usable \
         FROM pg_index i \
         JOIN pg_class c ON c.oid = i.indexrelid \
         JOIN pg_am am ON am.oid = c.relam \
         WHERE i.indrelid = $1 \
         ORDER BY 1",
    )
    .bind(oid)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            let name: String = get(row, "name")?;
            let definition: String = get(row, "definition")?;
            let key = match recognise(&definition) {
                Ok(DdlStatement::CreateIndex { key, .. }) => Some(key),
                Ok(other) => {
                    warn!(table, index = %name, ?other, "index definition read as another statement");
                    None
                }
                Err(error) => {
                    warn!(table, index = %name, %error, "index key does not read as a key list");
                    None
                }
            };
            Ok(CatalogIndex {
                key,
                definition,
                primary: get(row, "is_primary")?,
                unique: get(row, "is_unique")?,
                partial: get(row, "partial")?,
                condition: row
                    .try_get::<Option<String>, _>("condition")
                    .ok()
                    .flatten()
                    .map(|condition| unwrapped(&condition).to_owned()),
                expressions: get(row, "expressions")?,
                includes: get(row, "includes")?,
                include: get(row, "include")?,
                method: get(row, "method")?,
                usable: get(row, "usable")?,
                name,
            })
        })
        .collect()
}

async fn constraints(pool: &PgPool, oid: Oid) -> Result<Vec<CatalogConstraint>, SqlError> {
    let rows = sqlx::query(
        "SELECT conname::text AS name, contype::text AS kind, conrelid::regclass::text AS on_table, \
                pg_get_constraintdef(oid) AS definition \
         FROM pg_constraint WHERE conrelid = $1 OR confrelid = $1 ORDER BY 1",
    )
    .bind(oid)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            let kind: String = get(row, "kind")?;
            Ok(CatalogConstraint {
                name: get(row, "name")?,
                table: get(row, "on_table")?,
                kind: kind.chars().next().unwrap_or('?'),
                definition: get(row, "definition")?,
            })
        })
        .collect()
}

/// The connected role's reach over the table's shape, across every role it is a `MEMBER` of.
async fn access(pool: &PgPool, oid: Oid) -> Result<RoleAccess, SqlError> {
    let row = sqlx::query(
        "WITH member AS ( \
             SELECT r.oid, r.rolname, r.rolsuper, r.rolcreaterole, r.rolbypassrls \
             FROM pg_roles r WHERE pg_has_role(current_user, r.oid, 'MEMBER')) \
         SELECT pg_has_role(current_user, c.relowner, 'MEMBER') AS owns, \
                EXISTS (SELECT 1 FROM member WHERE rolsuper) AS superuser, \
                EXISTS (SELECT 1 FROM member WHERE rolcreaterole) AS create_role, \
                EXISTS (SELECT 1 FROM member WHERE rolbypassrls) AS bypass_row_security, \
                EXISTS (SELECT 1 FROM member WHERE rolname = 'yb_db_admin') AS db_admin, \
                EXISTS (SELECT 1 FROM member m \
                        WHERE has_schema_privilege(m.oid, c.relnamespace, 'CREATE')) \
                    AS create_in_schema, \
                EXISTS (SELECT 1 FROM member m WHERE has_table_privilege(m.oid, c.oid, 'TRIGGER')) \
                    AS can_trigger, \
                EXISTS (SELECT 1 FROM member m WHERE has_table_privilege(m.oid, c.oid, 'TRUNCATE')) \
                    AS can_truncate, \
                EXISTS (SELECT 1 FROM member m \
                        WHERE has_table_privilege(m.oid, c.oid, 'REFERENCES')) AS can_reference, \
                EXISTS (SELECT 1 FROM pg_proc p \
                        JOIN pg_namespace n ON n.oid = p.pronamespace \
                        JOIN pg_roles o ON o.oid = p.proowner \
                        WHERE p.prosecdef \
                          AND n.nspname NOT IN ('pg_catalog', 'information_schema') \
                          AND (o.rolsuper OR pg_has_role(p.proowner, c.relowner, 'MEMBER')) \
                          AND has_function_privilege(current_user, p.oid, 'EXECUTE')) \
                    AS definer_function \
         FROM pg_class c WHERE c.oid = $1",
    )
    .bind(oid)
    .fetch_one(pool)
    .await?;
    Ok(RoleAccess {
        owns: get(&row, "owns")?,
        superuser: get(&row, "superuser")?,
        create_role: get(&row, "create_role")?,
        bypass_row_security: get(&row, "bypass_row_security")?,
        db_admin: get(&row, "db_admin")?,
        create_in_schema: get(&row, "create_in_schema")?,
        trigger: get(&row, "can_trigger")?,
        truncate: get(&row, "can_truncate")?,
        references: get(&row, "can_reference")?,
        definer_function: get(&row, "definer_function")?,
    })
}

async fn readers(pool: &PgPool, oid: Oid) -> Result<BTreeMap<String, String>, SqlError> {
    let rows = sqlx::query(
        "SELECT r.rolname::text AS role, pg_get_expr(p.polqual, p.polrelid) AS seen \
         FROM pg_policy p JOIN pg_roles r ON r.oid = p.polroles[1] \
         WHERE p.polrelid = $1 AND NOT p.polpermissive AND p.polcmd = 'r' \
           AND cardinality(p.polroles) = 1 AND p.polqual IS NOT NULL \
           AND has_table_privilege(r.oid, p.polrelid, 'SELECT') \
         ORDER BY 1",
    )
    .bind(oid)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            let seen: String = get(row, "seen")?;
            Ok((get(row, "role")?, unwrapped(&seen).to_owned()))
        })
        .collect()
}

async fn names(pool: &PgPool, oid: Oid, sql: &'static str) -> Result<Vec<String>, SqlError> {
    let rows = sqlx::query(sql).bind(oid).fetch_all(pool).await?;
    rows.iter().map(|row| get(row, "name")).collect()
}

fn get<'r, T>(row: &'r PgRow, column: &str) -> Result<T, SqlError>
where
    T: sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    row.try_get(column)
        .map_err(|e| SqlError::Introspection(format!("catalog column {column}: {e}")))
}

fn unwrapped(condition: &str) -> &str {
    condition
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .unwrap_or(condition)
}
