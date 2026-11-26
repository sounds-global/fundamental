use std::time::Duration;

use fundamental::web::{Cache, PagePolicy};

pub fn policy() -> PagePolicy {
    PagePolicy::new()
        .cache(Cache::Public {
            max_age: Duration::from_secs(300),
        })
        .title("Fixture")
}
