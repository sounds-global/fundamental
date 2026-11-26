//! A load run: its steps, latencies, throughput and bounds.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use fundamental_bench::{BenchError, Load, Report, Step};
use fundamental_core::Error as _;

fn quick(name: &str) -> Load {
    Load::new(name)
        .concurrency([1, 4])
        .each_for(Duration::from_millis(60))
        .warm_for(Duration::ZERO)
}

#[tokio::test]
async fn each_concurrency_is_a_step_of_the_report() {
    let report = quick("noop")
        .run(|| async { Ok::<(), String>(()) })
        .await
        .expect("a report");
    let concurrencies: Vec<usize> = report.steps().iter().map(|step| step.concurrency).collect();
    assert_eq!(concurrencies, vec![1, 4]);
    assert!(report.steps().iter().all(|step| step.requests > 0));
    assert!(report.steps().iter().all(|step| step.errors == 0));
}

fn every_other(call: u64) -> Result<(), String> {
    if call.is_multiple_of(2) {
        Err("refused".to_owned())
    } else {
        Ok(())
    }
}

#[tokio::test]
async fn a_failed_call_is_counted_as_an_error() {
    let calls = Arc::new(AtomicU64::new(0));
    let report = quick("half")
        .run(move || {
            let calls = calls.clone();
            async move { every_other(calls.fetch_add(1, Ordering::Relaxed)) }
        })
        .await
        .expect("a report");
    let step = &report.steps()[0];
    assert!(step.errors > 0, "{step:?}");
    let error = report.error_free().expect_err("errors");
    assert_eq!(error.error_code(), "BENCH_006");
}

#[tokio::test]
async fn the_percentiles_are_ordered() {
    let report = quick("sleep")
        .run(|| async {
            tokio::time::sleep(Duration::from_millis(2)).await;
            Ok::<(), String>(())
        })
        .await
        .expect("a report");
    for step in report.steps() {
        assert!(step.p50_us >= 2_000, "{step:?}");
        assert!(step.p50_us <= step.p95_us && step.p95_us <= step.p99_us);
        assert!(step.p99_us <= step.max_us);
    }
}

fn step(concurrency: usize, p99_us: u64) -> Step {
    Step {
        concurrency,
        seconds: 1.0,
        requests: 10,
        errors: 0,
        per_second: 10.0,
        p50_us: 1,
        p95_us: 1,
        p99_us,
        max_us: p99_us,
    }
}

#[test]
fn a_step_beyond_its_p99_bound_is_named() {
    let report = Report::new("pages", vec![step(1, 900), step(32, 70_000)]);
    let error = report
        .p99_below(Duration::from_millis(50))
        .expect_err("the second step misses");
    assert_eq!(error.error_code(), "BENCH_007");
    let BenchError::Missed { listed, .. } = &error else {
        panic!("missed, not {error:?}");
    };
    assert!(listed.contains("concurrency 32"), "{listed}");
    assert!(!listed.contains("concurrency 1 "), "{listed}");
}

#[test]
fn a_report_is_written_as_json_and_read_back() {
    let dir = tempfile::tempdir().expect("a directory");
    let path = dir.path().join("nested").join("pages.json");
    let report = Report::new("pages", vec![step(8, 1_000)]);
    report.write(&path).expect("written");
    let text = std::fs::read_to_string(&path).expect("read");
    let back: Report = serde_json::from_str(&text).expect("json");
    assert_eq!(back, report);
}

#[test]
fn a_report_renders_as_a_markdown_table() {
    let table = Report::new("pages", vec![step(8, 1_500)]).table();
    assert!(table.starts_with("| concurrency |"), "{table}");
    assert!(table.contains("| 8 |"), "{table}");
    assert!(table.contains("1.50"), "{table}");
}
