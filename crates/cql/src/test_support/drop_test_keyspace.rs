use std::time::Duration;

use crate::{CqlConfig, CqlPool};

/// How long a teardown may spend removing a keyspace before it gives up and says so.
///
/// Finite: an unbounded teardown inside a `Drop` that `join`s its thread can hang the suite.
const TEARDOWN_BUDGET: Duration = Duration::from_secs(30);

/// Drop a test's keyspace, from inside a `Drop` impl, without deadlocking the test.
///
/// It takes a config, not a pool: a pooled connection belongs to the test's current-thread
/// runtime, which is parked while `Drop` joins. It connects inside its own runtime, is bounded,
/// and never panics, as a `Drop` may run while a test is already unwinding.
///
/// # Example
///
/// ```no_run
/// # use fundamental_cql::CqlConfig;
/// # use fundamental_cql::test_support::drop_test_keyspace;
/// struct TestKeyspace {
///     config: CqlConfig,
///     keyspace: String,
/// }
///
/// impl Drop for TestKeyspace {
///     fn drop(&mut self) {
///         drop_test_keyspace(&self.config, &self.keyspace);
///     }
/// }
/// ```
pub fn drop_test_keyspace(config: &CqlConfig, keyspace: &str) {
    let config = CqlConfig {
        default_keyspace: None,
        ..config.clone()
    };
    let name = keyspace.to_owned();
    let teardown = std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            eprintln!(
                "teardown: runtime build failed, so DROP KEYSPACE {name} was never issued; \
                 the acquire-time sweep reclaims it on its age bound"
            );
            return;
        };
        runtime.block_on(async {
            let dropped = tokio::time::timeout(TEARDOWN_BUDGET, async {
                let pool = CqlPool::connect(&config).await?;
                // Tables first, then the keyspace: see `drop_keyspace_with_tables`.
                super::drop_keyspace_with_tables(&pool, &name).await?;
                Ok::<_, crate::CqlError>(())
            })
            .await;
            match dropped {
                Ok(Ok(())) => {}
                Ok(Err(error)) => eprintln!("teardown: DROP KEYSPACE {name} failed: {error}"),
                // Not leaked: the statement was issued, and the acquire-time sweep finishes what a
                // bound cannot.
                Err(_) => eprintln!(
                    "teardown: DROP KEYSPACE {name} was issued and had not finished within \
                     {}s; the acquire-time sweep reclaims it if the engine did not",
                    TEARDOWN_BUDGET.as_secs()
                ),
            }
        });
    });
    if teardown.join().is_err() {
        eprintln!(
            "teardown: thread panicked while dropping {keyspace}; \
             the acquire-time sweep reclaims it on its age bound"
        );
    }
}
