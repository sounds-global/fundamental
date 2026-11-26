//! A fault run: every acknowledged write found again exactly once.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fundamental_bench::{Fault, FaultReport};
use fundamental_core::Error as _;

/// A store that refuses writes while `down`, and holds every accepted one by number.
#[derive(Clone, Default)]
struct Store {
    down: Arc<AtomicBool>,
    rows: Arc<Mutex<BTreeMap<u64, u32>>>,
}

impl Store {
    fn write(&self, n: u64) -> Result<(), String> {
        if self.down.load(Ordering::SeqCst) {
            return Err("the store is down".to_owned());
        }
        *self.rows.lock().expect("rows").entry(n).or_default() += 1;
        Ok(())
    }

    fn rows(&self) -> Vec<u64> {
        let rows = self.rows.lock().expect("rows");
        rows.iter()
            .flat_map(|(n, times)| std::iter::repeat_n(*n, *times as usize))
            .collect()
    }
}

fn fault() -> Fault {
    Fault::new("outage")
        .writes(40)
        .pace(Duration::from_millis(2))
        .inject_after(Duration::from_millis(10))
        .retry_every(Duration::from_millis(5))
        .give_up_after(Duration::from_secs(5))
}

#[tokio::test]
async fn writes_retried_through_an_outage_all_survive() {
    let store = Store::default();
    let writer = store.clone();
    let outage = store.clone();
    let report = fault()
        .run(
            move |n| {
                let writer = writer.clone();
                async move { writer.write(n) }
            },
            async move {
                outage.down.store(true, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(40)).await;
                outage.down.store(false, Ordering::SeqCst);
                Ok::<(), String>(())
            },
            move || async move { Ok::<_, String>(store.rows()) },
        )
        .await
        .expect("a report");
    report.judge().expect("no write lost");
    assert_eq!(report.acknowledged, 40);
    assert!(report.failed_attempts > 0, "the outage refused writes");
    assert!(report.outage_secs.expect("an outage was seen") >= 0.03);
}

fn judged(acknowledged: &[u64], found: &[u64]) -> FaultReport {
    FaultReport::of("outage", 3, acknowledged, found)
}

#[test]
fn an_acknowledged_write_that_is_missing_is_lost() {
    let report = judged(&[0, 1, 2], &[0, 2]);
    assert_eq!(report.lost, vec![1]);
    assert_eq!(report.judge().expect_err("lost").error_code(), "BENCH_011");
}

#[test]
fn a_write_found_twice_is_duplicated() {
    let report = judged(&[0, 1, 2], &[0, 1, 1, 2]);
    assert_eq!(report.duplicated, vec![1]);
    assert!(report.judge().is_err());
}

#[test]
fn a_write_never_acknowledged_fails_the_run() {
    let report = judged(&[0, 2], &[0, 2]);
    assert_eq!(report.unacknowledged, vec![1]);
    assert!(report.judge().is_err());
}

#[test]
fn every_write_found_once_holds() {
    judged(&[0, 1, 2], &[2, 0, 1]).judge().expect("held");
}
