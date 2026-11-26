//! The `#[seed]` macro feeding a [`SeedRunner`] against an offline SQLite backend.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Mutex;

use fundamental_db::{KeyedRepository, Repository, RepositoryAggregate};
use fundamental_macros::{Model, seed};
use fundamental_offline::SharedOfflineDatabase;
use fundamental_offline::db::{OfflineConfig, OfflineDatabase, Value};
use fundamental_offline::store::OfflineStore;
use fundamental_seed::{SeedError, SeedRunner};
use tracing_test::traced_test;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(offline)]
struct Widget {
    #[model(id)]
    id: String,
    name: String,
}

struct Ctx {
    db: SharedOfflineDatabase,
    order: Mutex<Vec<&'static str>>,
}

impl Ctx {
    fn ran(&self) -> Vec<&'static str> {
        self.order.lock().expect("order lock").clone()
    }
}

async fn setup() -> Ctx {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("open in-memory db");
    OfflineStore::new(&db)
        .ensure_schema::<Widget>()
        .expect("ensure schema");
    Ctx {
        db: db.into_shared(),
        order: Mutex::new(Vec::new()),
    }
}

#[seed(order = 10)]
async fn bootstrap(ctx: &Ctx) -> Result<(), SeedError> {
    ctx.order.lock().expect("order lock").push("bootstrap");
    ctx.db
        .repository::<Widget>()
        .upsert(&Widget {
            id: "w1".into(),
            name: "first".into(),
        })
        .await?;
    Ok(())
}

#[seed(after = "bootstrap")]
async fn demo(ctx: &Ctx) -> Result<(), SeedError> {
    ctx.order.lock().expect("order lock").push("demo");
    ctx.db
        .repository::<Widget>()
        .upsert(&Widget {
            id: "w2".into(),
            name: "second".into(),
        })
        .await?;
    Ok(())
}

#[seed(after = "ghost")]
async fn orphan(ctx: &Ctx) -> Result<(), SeedError> {
    ctx.order.lock().expect("order lock").push("orphan");
    Ok(())
}

#[tokio::test]
async fn runs_seeds_in_dependency_order_and_is_idempotent() {
    let ctx = setup().await;
    let runner = SeedRunner::new()
        .register(demo_seed())
        .register(bootstrap_seed());

    runner.run_all(&ctx).await.expect("seeding succeeds");

    assert_eq!(ctx.ran(), vec!["bootstrap", "demo"]);
    let repo = ctx.db.repository::<Widget>();
    assert_eq!(repo.count().await.unwrap(), 2);
    assert_eq!(
        repo.find(&Value::from("w1")).await.unwrap().unwrap().name,
        "first"
    );

    ctx.order.lock().unwrap().clear();
    runner.run_all(&ctx).await.expect("re-seeding succeeds");
    assert_eq!(ctx.ran(), vec!["bootstrap", "demo"]);
    assert_eq!(repo.count().await.unwrap(), 2);
}

#[traced_test]
#[tokio::test]
async fn emits_lifecycle_events_and_warns_on_missing_dependency() {
    let ctx = setup().await;
    let runner = SeedRunner::new()
        .register(bootstrap_seed())
        .register(orphan_seed());

    runner.run_all(&ctx).await.expect("seeding succeeds");

    assert_eq!(ctx.ran(), vec!["bootstrap", "orphan"]);

    assert!(
        logs_contain("running seeds"),
        "runner logs how many it runs"
    );
    assert!(logs_contain("seed complete"), "each seed logs completion");
    assert!(
        logs_contain("ignoring the dependency"),
        "a missing `after` is warned, not silent"
    );
}

#[tokio::test]
async fn a_failing_seed_aborts_the_run_and_propagates_its_error() {
    #[seed]
    async fn explode(_ctx: &Ctx) -> Result<(), SeedError> {
        Err(SeedError::msg("intentional failure"))
    }

    let ctx = setup().await;
    let runner = SeedRunner::new().register(explode_seed());
    let err = runner.run_all(&ctx).await.expect_err("the seed fails");
    assert!(matches!(err, SeedError::Message { .. }));
}
