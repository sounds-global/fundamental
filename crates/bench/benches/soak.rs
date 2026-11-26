//! The load mix held at one concurrency while resident memory is sampled; fails on any failed
//! call or memory growth beyond 10%. Run with `fun bench soak --duration 10m`.

#[cfg(not(debug_assertions))]
compile_error!(
    "the live benches run on test containers, whose fixtures need debug assertions: \
     run `fun bench soak` (cargo bench --profile bench-live --features live --bench soak)"
);

use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use fundamental_bench::LiveArgs;
use fundamental_bench::Soak;
use fundamental_bench::live::{BenchApp, Engines, out_dir};
use fundamental_core::AnyError;
use fundamental_test_containers::container_name::NATS_09_BENCH;
use fundamental_test_containers::nats::NatsServer;
use fundamental_test_containers::sql::SqlDatabase;

/// How many calls are in flight, unless `--concurrency` names one.
const CONCURRENCY: usize = 32;

#[tokio::main]
async fn main() -> ExitCode {
    install_logging();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("soak: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), AnyError> {
    let args = LiveArgs::from_env().map_err(|reason| {
        AnyError::new(fundamental_bench::BenchError::Boot {
            step: "read its settings".to_owned(),
            cause: format!("{reason}; settings: --duration 10m --concurrency 32"),
        })
    })?;
    let concurrency = match args.concurrency.as_slice() {
        [one] => *one,
        _ => CONCURRENCY,
    };
    let database = SqlDatabase::mint().await?;
    let nats = NatsServer::shared(NATS_09_BENCH).await?;
    let engines = Engines {
        sql_url: database.url().to_owned(),
        nats: nats.config(),
        max_connections: 64,
    };
    let app = Arc::new(BenchApp::boot(&engines, 8).await?);
    let turn = Arc::new(AtomicU64::new(0));
    let driven = Arc::clone(&app);
    let report = Soak::new("mix")
        .concurrency(concurrency)
        .for_duration(args.duration)
        .run(move || {
            let app = Arc::clone(&driven);
            let turn = turn.fetch_add(1, Ordering::Relaxed);
            async move {
                match turn % 4 {
                    0 => app.page().await,
                    1 => app.next_write().await,
                    2 => app.worker_tick().await,
                    _ => app.pull().await,
                }
            }
        })
        .await?;
    report.write(out_dir().join("soak").join("mix.json"))?;
    let first = report.samples.first().map_or(0, |(_, bytes)| *bytes);
    let last = report.samples.last().map_or(0, |(_, bytes)| *bytes);
    println!(
        "soak mix: {} calls ({} failed) at {concurrency} for {:.0} s; resident {} MiB -> {} MiB; growth {:.1}% (limit {:.0}%)",
        report.load.requests,
        report.load.errors,
        report.load.seconds,
        first / (1024 * 1024),
        last / (1024 * 1024),
        report.growth().unwrap_or_default() * 100.0,
        report.limit * 100.0,
    );
    report.judge()?;
    Ok(())
}

fn install_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,fundamental_bench=info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}
