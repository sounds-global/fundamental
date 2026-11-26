use std::time::Duration;

use crate::{SqlConfig, SqlPool};

/// How long a teardown may spend removing a database; one uncontended `DROP DATABASE` takes up to a minute here.
const TEARDOWN_BUDGET: Duration = Duration::from_secs(120);

/// Drop a test's database from inside a `Drop` impl, without deadlocking the test.
///
/// Takes a URL, never a live pool: a pool's connections belong to the test's current-thread
/// runtime, which is parked while `Drop` waits, so a statement on them never wakes. `WITH
/// (FORCE)` ends the sessions a pool has not yet closed. Never panics; problems go to stderr.
///
/// # Example
///
/// ```no_run
/// # use fundamental_sql::test_support::drop_test_database;
/// struct TestDatabase {
///     admin_url: String,
///     database: String,
/// }
///
/// impl Drop for TestDatabase {
///     fn drop(&mut self) {
///         drop_test_database(&self.admin_url, &self.database);
///     }
/// }
/// ```
pub fn drop_test_database(admin_url: &str, database: &str) {
    let url = admin_url.to_owned();
    let name = database.to_owned();
    let teardown = std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            eprintln!(
                "teardown: runtime build failed, so DROP {name} was never issued; \
                 the acquire-time sweep reclaims it on its age bound"
            );
            return;
        };
        runtime.block_on(async {
            let dropped = tokio::time::timeout(TEARDOWN_BUDGET, async {
                let pool = SqlPool::connect(&SqlConfig::development(url.as_str())).await?;
                // `execute_ddl`: the drop contends with other fixtures' DDL; `IF EXISTS` makes a retry safe.
                pool.execute_ddl(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
                    .await
            })
            .await;
            match dropped {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => eprintln!("teardown: DROP {name} failed: {error}"),
                // Not "leaked": the engine finishes a drop nobody waits for, and the sweep reclaims a real leak.
                Err(_) => eprintln!(
                    "teardown: DROP {name} was issued and had not finished within {}s; \
                     the acquire-time sweep reclaims it if the engine did not",
                    TEARDOWN_BUDGET.as_secs()
                ),
            }
        });
    });
    if teardown.join().is_err() {
        eprintln!(
            "teardown: thread panicked while dropping {database}; \
             the acquire-time sweep reclaims it on its age bound"
        );
    }
}
