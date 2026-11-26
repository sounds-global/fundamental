//! The app on YugabyteDB and NATS at rising concurrency: a page, an action, a job worker and a
//! sync slice, into throughput and latency percentiles. Run with `fun bench load`.

#[cfg(not(debug_assertions))]
compile_error!(
    "the live benches run on test containers, whose fixtures need debug assertions: \
     run `fun bench load` (cargo bench --profile bench-live --features live --bench load)"
);

use std::future::Future;
use std::process::ExitCode;
use std::sync::Arc;

use fundamental_bench::LiveArgs;
use fundamental_bench::live::{BenchApp, Engines, out_dir};
use fundamental_bench::{Load, Report};
use fundamental_core::AnyError;
use fundamental_test_containers::container_name::NATS_09_BENCH;
use fundamental_test_containers::nats::NatsServer;
use fundamental_test_containers::sql::SqlDatabase;

/// The pool's ceiling: what one app process holds open on the database.
const MAX_CONNECTIONS: u32 = 64;

/// How many sync clients the app keeps connected and pulls through in turn.
const SYNC_CLIENTS: usize = 32;

#[tokio::main]
async fn main() -> ExitCode {
    install_logging();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("load: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), AnyError> {
    let args = LiveArgs::from_env().map_err(usage)?;
    let database = SqlDatabase::mint().await?;
    let nats = NatsServer::shared(NATS_09_BENCH).await?;
    let engines = Engines {
        sql_url: database.url().to_owned(),
        nats: nats.config(),
        max_connections: MAX_CONNECTIONS,
    };
    let app = Arc::new(BenchApp::boot(&engines, SYNC_CLIENTS).await?);
    let mut reports = Vec::new();
    // The actions last: the job backlog they leave is not the worker's own load.
    reports.push(step("pages", &args, &app, |app| async move { app.page().await }).await?);
    reports.push(
        step("worker", &args, &app, |app| async move {
            app.worker_tick().await
        })
        .await?,
    );
    reports.push(step("sync", &args, &app, |app| async move { app.pull().await }).await?);
    reports.push(
        step("actions", &args, &app, |app| async move {
            app.next_write().await
        })
        .await?,
    );
    for report in &reports {
        println!("\n### {}\n\n{}", report.name, report.table());
    }
    if let Ok(app) = Arc::try_unwrap(app) {
        app.stop().await?;
    }
    for report in &reports {
        report.error_free()?;
    }
    Ok(())
}

/// Drives `call` at each concurrency of `args` and writes the report.
async fn step<F, Fut>(
    name: &str,
    args: &LiveArgs,
    app: &Arc<BenchApp>,
    call: F,
) -> Result<Report, AnyError>
where
    F: Fn(Arc<BenchApp>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), String>> + Send + 'static,
{
    let app = Arc::clone(app);
    let report = Load::new(name)
        .concurrency(args.concurrency.iter().copied())
        .each_for(args.each)
        .run(move || call(Arc::clone(&app)))
        .await?;
    report.write(out_dir().join("load").join(format!("{name}.json")))?;
    Ok(report)
}

fn usage(reason: String) -> AnyError {
    AnyError::new(fundamental_bench::BenchError::Boot {
        step: "read its settings".to_owned(),
        cause: format!("{reason}; settings: --each 20s --concurrency 1,8,32,128"),
    })
}

fn install_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,fundamental_bench=info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}
