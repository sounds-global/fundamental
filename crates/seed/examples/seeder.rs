//! A minimal seeder binary built with `seed_main!`, proving its generated `main` compiles.

use fundamental_macros::{seed, seed_main};
use fundamental_seed::SeedError;

struct AppContext;

#[seed(order = 10)]
async fn bootstrap(_ctx: &AppContext) -> Result<(), SeedError> {
    Ok(())
}

#[seed(after = "bootstrap")]
async fn demo(_ctx: &AppContext) -> Result<(), SeedError> {
    Ok(())
}

seed_main! {
    app = fundamental_config::AppSettings::load("main")?,
    context = AppContext,
    seeds = [bootstrap, demo],
}
