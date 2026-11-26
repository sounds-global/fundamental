//! The committed baseline and its check.

use fundamental_bench::{Baseline, BenchError, Finding, Measured};
use fundamental_core::Error as _;

fn baseline(json: &str) -> Baseline {
    Baseline::from_json(json).expect("a baseline")
}

const ONE: &str = r#"{ "arch": "aarch64", "benches": { "render/page": { "ir": 1000 } } }"#;

#[test]
fn a_count_within_the_threshold_passes() {
    let measured = Measured::from_iter([("render/page", 1_019)]);
    let checked = baseline(ONE).check(&measured).expect("within 2%");
    assert_eq!(checked.compared, 1);
}

#[test]
fn a_count_beyond_the_threshold_is_a_regression() {
    let measured = Measured::from_iter([("render/page", 1_021)]);
    let error = baseline(ONE).check(&measured).expect_err("beyond 2%");
    assert_eq!(error.error_code(), "BENCH_001");
    let BenchError::Regressed { findings, .. } = error else {
        panic!("a regression, not {error:?}");
    };
    assert_eq!(
        findings,
        vec![Finding::Regressed {
            bench: "render/page".to_owned(),
            was: 1_000,
            now: 1_021,
            limit: 0.02,
        }]
    );
}

#[test]
fn every_regression_is_named_at_once() {
    let two =
        baseline(r#"{ "arch": "aarch64", "benches": { "a": { "ir": 100 }, "b": { "ir": 100 } } }"#);
    let measured = Measured::from_iter([("a", 200), ("b", 300)]);
    let BenchError::Regressed { findings, .. } = two.check(&measured).expect_err("both") else {
        panic!("a regression");
    };
    assert_eq!(findings.len(), 2);
}

#[test]
fn a_bench_threshold_overrides_the_default() {
    let loose = baseline(
        r#"{ "arch": "aarch64", "benches": { "render/page": { "ir": 1000, "threshold": 0.10 } } }"#,
    );
    let measured = Measured::from_iter([("render/page", 1_090)]);
    assert!(loose.check(&measured).is_ok());
}

#[test]
fn the_default_threshold_is_the_files_own_when_it_names_one() {
    let strict = baseline(
        r#"{ "arch": "aarch64", "threshold": 0.001, "benches": { "render/page": { "ir": 1000 } } }"#,
    );
    let measured = Measured::from_iter([("render/page", 1_002)]);
    assert!(strict.check(&measured).is_err());
}

#[test]
fn a_faster_count_passes_and_is_reported() {
    let measured = Measured::from_iter([("render/page", 500)]);
    let checked = baseline(ONE).check(&measured).expect("faster passes");
    assert_eq!(checked.improved, vec!["render/page".to_owned()]);
}

#[test]
fn a_bench_missing_from_either_side_is_a_finding() {
    let measured = Measured::from_iter([("render/other", 10)]);
    let BenchError::Regressed { findings, .. } = baseline(ONE).check(&measured).expect_err("both")
    else {
        panic!("findings");
    };
    assert!(findings.contains(&Finding::NotMeasured {
        bench: "render/page".to_owned()
    }));
    assert!(findings.contains(&Finding::NotInBaseline {
        bench: "render/other".to_owned()
    }));
}

#[test]
fn saving_takes_the_measured_counts_and_keeps_each_threshold() {
    let loose = baseline(
        r#"{ "arch": "aarch64", "benches": { "render/page": { "ir": 1000, "threshold": 0.10 } } }"#,
    );
    let measured = Measured::from_iter([("render/page", 1_500), ("sync/encode", 7)]);
    let saved = loose.saved(&measured);
    assert!(saved.check(&measured).is_ok());
    let text = saved.to_json();
    assert!(text.contains("\"threshold\": 0.1"), "{text}");
    assert!(!text.contains("sync/encode\": {\n      \"ir\": 7,\n      \"threshold\""));
}

#[test]
fn the_baseline_is_read_for_this_machines_architecture() {
    let dir = tempfile::tempdir().expect("a directory");
    let path = dir.path().join(format!("{}.json", std::env::consts::ARCH));
    std::fs::write(&path, ONE.replace("aarch64", std::env::consts::ARCH)).expect("written");
    let read = Baseline::read(dir.path()).expect("read");
    assert_eq!(read.arch(), std::env::consts::ARCH);
}

#[test]
fn no_baseline_for_this_architecture_is_refused_with_its_path() {
    let dir = tempfile::tempdir().expect("a directory");
    let error = Baseline::read(dir.path()).expect_err("none");
    assert_eq!(error.error_code(), "BENCH_002");
    assert!(
        error.to_string().contains(std::env::consts::ARCH),
        "{error}"
    );
}

#[test]
fn a_baseline_for_another_architecture_is_refused() {
    let dir = tempfile::tempdir().expect("a directory");
    let path = dir.path().join(format!("{}.json", std::env::consts::ARCH));
    std::fs::write(&path, ONE.replace("aarch64", "riscv64")).expect("written");
    let error = Baseline::read(dir.path()).expect_err("another architecture");
    assert_eq!(error.error_code(), "BENCH_003");
}

#[test]
fn a_baseline_that_is_not_json_is_refused() {
    let error = Baseline::from_json("not json").expect_err("not json");
    assert_eq!(error.error_code(), "BENCH_003");
}

/// A gungraun summary as `--save-summary=json` writes it, cut to what is read.
fn summary(ir: u64) -> String {
    format!(
        r#"{{ "version": "7", "group": "render", "function_name": "page", "id": "fifty_rows",
             "profiles": [ {{ "tool": "Callgrind", "data": {{ "total": {{ "metrics": {{
                 "Ir": {{ "values": {{ "new": {ir} }} }},
                 "EstimatedCycles": {{ "values": {{ "new": 1 }} }} }}, "regressions": [] }} }} }} ] }}"#
    )
}

#[test]
fn every_summary_under_a_directory_is_measured_by_its_path() {
    let dir = tempfile::tempdir().expect("a directory");
    for (path, ir) in [
        ("app/instructions/render/page.fifty_rows", 188_069),
        ("app/instructions/sync/canonical.one_row", 2_814),
    ] {
        let at = dir.path().join(path);
        std::fs::create_dir_all(&at).expect("made");
        std::fs::write(at.join("summary.json"), summary(ir)).expect("written");
    }
    let measured = Measured::read_summaries(dir.path()).expect("read");
    assert_eq!(
        measured.get("app/instructions/render/page.fifty_rows"),
        Some(188_069)
    );
    assert_eq!(
        measured.get("app/instructions/sync/canonical.one_row"),
        Some(2_814)
    );
    assert_eq!(measured.len(), 2);
}

#[test]
fn a_summary_without_an_instruction_count_is_refused() {
    let dir = tempfile::tempdir().expect("a directory");
    let at = dir.path().join("app/instructions/render/page");
    std::fs::create_dir_all(&at).expect("made");
    std::fs::write(at.join("summary.json"), r#"{ "profiles": [] }"#).expect("written");
    let error = Measured::read_summaries(dir.path()).expect_err("no Ir");
    assert_eq!(error.error_code(), "BENCH_003");
}

#[test]
fn a_directory_with_no_summary_measures_nothing() {
    let dir = tempfile::tempdir().expect("a directory");
    assert!(
        Measured::read_summaries(dir.path())
            .expect("read")
            .is_empty()
    );
}
