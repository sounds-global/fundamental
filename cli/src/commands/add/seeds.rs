//! `fun add seeds`: create a seeds directory with a sample `#[seed]` stub.

use super::AddSeedsArgs;
use crate::helpers::{Project, write_file};
use crate::{CliResult, CommandContext};

/// Create `apps/<app>/seeds/` with a sample seed and a README on wiring a seeder binary.
///
/// # Errors
/// A [`CliError`](crate::CliError) if the project cannot be found or a file cannot be written.
pub(crate) fn run(args: AddSeedsArgs, ctx: CommandContext) -> CliResult<()> {
    let project = Project::discover()?;
    let seeds_dir = project.app_dir(&args.app).join("seeds");

    write_file(&seeds_dir.join("example.rs"), EXAMPLE_SEED)?;
    write_file(&seeds_dir.join("README.md"), SEEDS_README)?;

    let ui = ctx.ui();
    ui.blank();
    ui.section(format!("Created seeds for {}", args.app));
    ui.detail(format!("apps/{}/seeds/example.rs", args.app));
    ui.blank();
    ui.section("Next steps");
    ui.note("wire your seeds into a seeder bin with seed_main! (see seeds/README.md)");
    ui.command("fun seed");
    ui.blank();
    Ok(())
}

const EXAMPLE_SEED: &str = r#"//! A sample seed. Rename it and add your own under this directory.

use fundamental::seed;
use fundamental::seed::SeedError;

/// Your application's context: the providers your seeds need.
struct AppContext;

/// The baseline rows your application needs; idempotent, run in `order`.
#[seed(order = 10)]
async fn example(ctx: &AppContext) -> Result<(), SeedError> {
    // ctx.db.repository::<User>().upsert(&admin).await?;
    let _ = ctx;
    Ok(())
}
"#;

const SEEDS_README: &str = r#"# Seeds

Each file here defines one or more seeds — idempotent async functions that
populate your database with baseline rows. Mark each with `#[seed]`:

```rust,ignore
#[seed(order = 10)]
async fn example(ctx: &AppContext) -> Result<(), SeedError> {
    Ok(())
}
```

Wire them into a seeder binary (e.g. `src/bin/seed.rs`) with `seed_main!`, which
assembles a `SeedRunner`, applies the environment gate, and runs every registered
seed in order. Then run them with `fun seed`.
"#;
