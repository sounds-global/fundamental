//! [`drop_keyspace_with_tables`] — the one statement sequence that removes a keyspace on
//! both of this framework's CQL engines.

use crate::{CqlPool, CqlResult};

/// Drops `keyspace` and everything in it.
///
/// The tables go first: YugabyteDB refuses `DROP KEYSPACE` on a keyspace with tables, and leaks it.
///
/// # Errors
/// Returns [`CqlError`](crate::CqlError) if a name is not a valid identifier or a statement
/// fails; a table listing that cannot be read still attempts the keyspace.
pub async fn drop_keyspace_with_tables(pool: &CqlPool, keyspace: &str) -> CqlResult<()> {
    let ident = crate::CqlIdent::new(keyspace)?;

    // A bound value, not an interpolated one: `cql!` interpolates a validated `CqlIdent`
    // and nothing else, which is the guarantee that makes a statement a type.
    let tables: Vec<(String,)> = pool
        .fetch_all(
            &crate::Cql::new("SELECT table_name FROM system_schema.tables WHERE keyspace_name = ?")
                .bind(crate::CqlValue::Text(keyspace.to_owned())),
        )
        .await
        .unwrap_or_default();
    for (table,) in tables {
        let table = crate::CqlIdent::new(&table)?;
        pool.execute_ddl(&crate::ddl!("DROP TABLE IF EXISTS {}.{}", ident, table))
            .await?;
    }

    pool.execute_ddl(&crate::ddl!("DROP KEYSPACE IF EXISTS {}", ident))
        .await?;
    Ok(())
}
