use std::time::Duration;

use super::is_inlinable;
use crate::{SqlConfig, SqlPool};

/// How long a release may take before it gives up and lets the lease expire; a one-row `UPDATE`.
const RELEASE_BUDGET: Duration = Duration::from_secs(30);

/// Hands a leased schema back to the pool from inside a `Drop` impl, without deadlocking the test.
///
/// An optimisation: the lease expires anyway. Takes a URL, as
/// [`drop_test_schema`](super::drop_test_schema) does, and never panics.
///
/// # Example
///
/// ```no_run
/// # use fundamental_sql::test_support::release_leased_schema;
/// struct Leased {
///     database_url: String,
///     schema: String,
/// }
///
/// impl Drop for Leased {
///     fn drop(&mut self) {
///         release_leased_schema(&self.database_url, &self.schema);
///     }
/// }
/// ```
pub fn release_leased_schema(database_url: &str, schema: &str) {
    if !is_inlinable(schema) {
        eprintln!("release: refusing to inline '{schema}'; its lease will expire instead");
        return;
    }
    let url = database_url.to_owned();
    let name = schema.to_owned();
    let release = std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            eprintln!(
                "release: runtime build failed, so {name} was not handed back; its lease \
                 expires on its own"
            );
            return;
        };
        runtime.block_on(async {
            let released = tokio::time::timeout(RELEASE_BUDGET, async {
                let pool = SqlPool::connect(&SqlConfig::development(url.as_str())).await?;
                // `now()`, not `NULL`, so "free" has one meaning: `leased_until < now()`.
                pool.execute(&format!(
                    "UPDATE public.fundamental_pooled_schemas \
                        SET leased_until = now() \
                      WHERE schema_name = '{name}'"
                ))
                .await
            })
            .await;
            match released {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => eprintln!(
                    "release: {name} was not handed back: {error}; its lease expires on its own"
                ),
                Err(_) => eprintln!(
                    "release: {name} had not been handed back within {}s; its lease expires \
                     on its own",
                    RELEASE_BUDGET.as_secs()
                ),
            }
        });
    });
    if release.join().is_err() {
        eprintln!(
            "release: thread panicked while handing back {schema}; its lease expires on its own"
        );
    }
}
