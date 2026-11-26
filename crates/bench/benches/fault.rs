//! Numbered writes through a database restart, a NATS outage and a partition: every
//! acknowledged write must be a row and an event exactly once, and every job must fire.
//!
//! The engines are the run's own, each behind a `TcpProxy`: a partition cuts it, and a restart
//! retargets it, as Docker publishes a restarted container on new ports. Run with
//! `fun bench fault`.

#[cfg(not(debug_assertions))]
compile_error!(
    "the live benches run on test containers, whose fixtures need debug assertions: \
     run `fun bench fault` (cargo bench --profile bench-live --features live --bench fault)"
);

use std::net::SocketAddr;
use std::process::ExitCode;
use std::time::Duration;

use fundamental_bench::LiveArgs;
use fundamental_bench::live::{BenchApp, Engines, out_dir};
use fundamental_bench::{BenchError, Fault, FaultReport};
use fundamental_core::AnyError;
use fundamental_nats::NatsConfig;
use fundamental_test_containers::nats::DedicatedNatsServer;
use fundamental_test_containers::{DedicatedEngineNode, TcpProxy};

/// How long NATS stays down, and the partition lasts.
const OUTAGE: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> ExitCode {
    install_logging();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fault: {error}");
            ExitCode::FAILURE
        }
    }
}

struct Faults {
    node: DedicatedEngineNode,
    nats: DedicatedNatsServer,
    sql: TcpProxy,
    bus: TcpProxy,
}

impl Faults {
    async fn start() -> Result<Self, AnyError> {
        let node = DedicatedEngineNode::start().await?;
        let nats = DedicatedNatsServer::start().await?;
        let sql = TcpProxy::start(local(node.sql_port())).await.map_err(io)?;
        let bus = TcpProxy::start(local(nats.port())).await.map_err(io)?;
        Ok(Self {
            node,
            nats,
            sql,
            bus,
        })
    }

    fn engines(&self) -> Engines {
        Engines {
            sql_url: format!(
                "postgresql://yugabyte@{}/yugabyte?sslmode=disable",
                self.sql.address()
            ),
            // Reconnecting, as a deployed app's client does.
            nats: NatsConfig::new(format!("nats://{}", self.bus.address())),
            max_connections: 16,
        }
    }
}

async fn run() -> Result<(), AnyError> {
    let args = LiveArgs::from_env().map_err(|reason| {
        AnyError::new(BenchError::Boot {
            step: "read its settings".to_owned(),
            cause: format!("{reason}; settings: --writes 300"),
        })
    })?;
    let mut faults = Faults::start().await?;
    let mut reports = Vec::new();

    let app = BenchApp::boot(&faults.engines(), 1).await?;
    let restart = async {
        faults.node.restart().await?;
        faults.sql.retarget(local(faults.node.sql_port()));
        Ok::<(), AnyError>(())
    };
    reports.push(survive(&app, "database restart", args.writes, restart).await?);
    app.stop().await?;

    let app = BenchApp::boot(&faults.engines(), 1).await?;
    let outage = async {
        faults.nats.stop().await?;
        tokio::time::sleep(OUTAGE).await;
        faults.nats.start_again().await?;
        faults.bus.retarget(local(faults.nats.port()));
        Ok::<(), AnyError>(())
    };
    reports.push(survive(&app, "nats outage", args.writes, outage).await?);
    app.stop().await?;

    let app = BenchApp::boot(&faults.engines(), 1).await?;
    let partition = async {
        faults.sql.cut();
        faults.bus.cut();
        tokio::time::sleep(OUTAGE).await;
        faults.sql.restore();
        faults.bus.restore();
        Ok::<(), AnyError>(())
    };
    reports.push(survive(&app, "network partition", args.writes, partition).await?);
    app.stop().await?;

    print_table(&reports);
    verdict(&reports)
}

fn print_table(reports: &[(FaultReport, u64)]) {
    println!(
        "\n| fault | writes | acknowledged | refused attempts | outage s | lost | duplicated | jobs fired |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|---:|");
    for (report, fired) in reports {
        println!(
            "| {} | {} | {} | {} | {:.1} | {} | {} | {fired} |",
            report.name,
            report.writes,
            report.acknowledged,
            report.failed_attempts,
            report.outage_secs.unwrap_or_default(),
            report.lost.len(),
            report.duplicated.len(),
        );
    }
}

/// Every acknowledged write kept exactly once, and every job fired.
fn verdict(reports: &[(FaultReport, u64)]) -> Result<(), AnyError> {
    for (report, fired) in reports {
        report.judge()?;
        if *fired != report.writes {
            return Err(AnyError::new(BenchError::WritesLost {
                run: report.name.clone(),
                lost: format!(
                    "{} of {} jobs never fired",
                    report.writes - fired,
                    report.writes
                ),
                duplicated: "none".to_owned(),
                unacknowledged: "none".to_owned(),
            }));
        }
    }
    Ok(())
}

/// Makes `writes` writes through `app` while `fault` runs, reads them back, fires every job,
/// and writes the report.
async fn survive(
    app: &BenchApp,
    name: &str,
    writes: u64,
    fault: impl Future<Output = Result<(), AnyError>>,
) -> Result<(FaultReport, u64), AnyError> {
    let report = Fault::new(name)
        .writes(writes)
        .inject_after(Duration::from_secs(1))
        .run(|n| app.write(n), fault, || app.written(writes))
        .await?;
    app.drain_all().await.map_err(boot)?;
    let fired = app.fired().await.map_err(boot)?;
    report.write(
        out_dir()
            .join("fault")
            .join(format!("{}.json", name.replace(' ', "_"))),
    )?;
    Ok((report, fired))
}

fn local(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

fn io(error: std::io::Error) -> AnyError {
    boot(error.to_string())
}

fn boot(cause: String) -> AnyError {
    AnyError::new(BenchError::Boot {
        step: "run a fault".to_owned(),
        cause,
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
