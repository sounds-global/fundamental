//! The settings a live run reads from the words after `--`.

use std::time::Duration;

use fundamental_bench::LiveArgs;

fn parse(words: &[&str]) -> Result<LiveArgs, String> {
    LiveArgs::parse(words.iter().map(|word| (*word).to_owned()))
}

#[test]
fn no_words_are_the_defaults() {
    let args = parse(&[]).expect("defaults");
    assert_eq!(args.each, Duration::from_secs(20));
    assert_eq!(args.concurrency, vec![1, 8, 32, 128]);
    assert_eq!(args.duration, Duration::from_secs(600));
    assert_eq!(args.writes, 300);
}

#[test]
fn each_setting_is_read_and_cargos_bench_flag_ignored() {
    let args = parse(&[
        "--each",
        "5s",
        "--concurrency",
        "2, 4",
        "--duration",
        "2m",
        "--writes",
        "50",
        "--bench",
    ])
    .expect("read");
    assert_eq!(args.each, Duration::from_secs(5));
    assert_eq!(args.concurrency, vec![2, 4]);
    assert_eq!(args.duration, Duration::from_secs(120));
    assert_eq!(args.writes, 50);
}

#[test]
fn a_bare_number_is_seconds_and_an_hour_is_read() {
    assert_eq!(
        parse(&["--each", "7"]).expect("read").each,
        Duration::from_secs(7)
    );
    assert_eq!(
        parse(&["--duration", "1h"]).expect("read").duration,
        Duration::from_secs(3_600)
    );
}

#[test]
fn an_unknown_word_or_a_bad_value_is_refused() {
    assert!(parse(&["--fast"]).is_err());
    assert!(parse(&["--each", "soon"]).is_err());
    assert!(parse(&["--each", "5d"]).is_err());
    assert!(parse(&["--concurrency", "a"]).is_err());
    assert!(parse(&["--writes"]).is_err());
}
