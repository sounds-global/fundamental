use std::time::Duration;

use crate::{SqlConfig, SqlPool};

/// How long a teardown may spend removing a schema before it gives up and says so.
const TEARDOWN_BUDGET: Duration = Duration::from_secs(120);

/// Drop a test's schema from inside a `Drop` impl, without deadlocking the test.
///
/// Takes a URL, as [`drop_test_database`](super::drop_test_database) does. `DROP SCHEMA …
/// CASCADE` waits on locks, not sessions, so it needs no `FORCE`. Never panics; problems go to stderr.
///
/// # Example
///
/// ```no_run
/// # use fundamental_sql::test_support::drop_test_schema;
/// struct TestSchema {
///     database_url: String,
///     schema: String,
/// }
///
/// impl Drop for TestSchema {
///     fn drop(&mut self) {
///         drop_test_schema(&self.database_url, &self.schema);
///     }
/// }
/// ```
pub fn drop_test_schema(database_url: &str, schema: &str) {
    let url = database_url.to_owned();
    let name = schema.to_owned();
    let teardown = std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            eprintln!(
                "teardown: runtime build failed, so DROP SCHEMA {name} was never issued; \
                 the acquire-time sweep reclaims it on its age bound"
            );
            return;
        };
        runtime.block_on(async {
            let dropped = tokio::time::timeout(TEARDOWN_BUDGET, async {
                let pool = SqlPool::connect(&SqlConfig::development(url.as_str())).await?;
                // `execute_ddl`: the drop contends with other fixtures' DDL; `IF EXISTS` makes a retry safe.
                pool.execute_ddl(&format!("DROP SCHEMA IF EXISTS {name} CASCADE"))
                    .await
            })
            .await;
            match dropped {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => eprintln!("teardown: DROP SCHEMA {name} failed: {error}"),
                // Not "leaked": the engine finishes a drop nobody waits for.
                Err(_) => eprintln!(
                    "teardown: DROP SCHEMA {name} was issued and had not finished within \
                     {}s; the acquire-time sweep reclaims it if the engine did not",
                    TEARDOWN_BUDGET.as_secs()
                ),
            }
        });
    });
    if teardown.join().is_err() {
        eprintln!(
            "teardown: thread panicked while dropping schema {schema}; \
             the acquire-time sweep reclaims it on its age bound"
        );
    }
}
