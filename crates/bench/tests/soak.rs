//! A soak run, judged on memory growth and on any failed call.

use std::time::Duration;

use fundamental_bench::{Soak, SoakReport, Step};
use fundamental_core::Error as _;

#[tokio::test]
async fn a_soak_samples_memory_while_the_load_runs() {
    let report = Soak::new("noop")
        .concurrency(2)
        .for_duration(Duration::from_millis(120))
        .sample_every(Duration::from_millis(20))
        .run(|| async {
            tokio::task::yield_now().await;
            Ok::<(), String>(())
        })
        .await
        .expect("a report");
    assert!(report.samples.len() >= 3, "{:?}", report.samples);
    assert!(report.load.requests > 0);
    assert_eq!(report.load.errors, 0);
}

fn report(rss: &[u64], errors: u64) -> SoakReport {
    SoakReport {
        name: "mix".to_owned(),
        limit: 0.10,
        samples: rss
            .iter()
            .enumerate()
            .map(|(at, bytes)| (at as f64 * 10.0, *bytes))
            .collect(),
        load: Step {
            concurrency: 8,
            seconds: 60.0,
            requests: 100,
            errors,
            per_second: 1.0,
            p50_us: 1,
            p95_us: 1,
            p99_us: 1,
            max_us: 1,
        },
    }
}

#[test]
fn flat_memory_and_no_error_hold() {
    let flat = report(&[100, 104, 98, 101, 99, 103], 0);
    flat.judge().expect("flat");
    assert!(flat.growth().expect("a growth") < 0.05);
}

#[test]
fn memory_that_grows_beyond_the_limit_fails() {
    let error = report(&[100, 100, 110, 120, 130, 140], 0)
        .judge()
        .expect_err("it grew");
    assert_eq!(error.error_code(), "BENCH_009");
}

#[test]
fn a_failed_call_fails_the_soak() {
    let error = report(&[100, 100, 100], 1).judge().expect_err("an error");
    assert_eq!(error.error_code(), "BENCH_006");
}

#[test]
fn fewer_than_three_samples_cannot_be_judged() {
    let error = report(&[100, 100], 0).judge().expect_err("too short");
    assert_eq!(error.error_code(), "BENCH_010");
}
