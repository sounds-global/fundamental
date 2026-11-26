//! The process's resident memory, read from `/proc` with no unsafe code.

use fundamental_bench::Rss;
use fundamental_core::Error as _;

#[test]
fn the_resident_set_is_read_from_the_status_file_in_bytes() {
    let status = "Name:\tbench\nVmPeak:\t  9000 kB\nVmRSS:\t  2048 kB\nThreads:\t4\n";
    assert_eq!(
        Rss::from_status(status).expect("a size").bytes(),
        2048 * 1024
    );
}

#[test]
fn a_status_without_a_resident_set_is_refused() {
    let error = Rss::from_status("Name:\tbench\n").expect_err("no VmRSS");
    assert_eq!(error.error_code(), "BENCH_005");
}

#[cfg(target_os = "linux")]
#[test]
fn this_process_has_a_resident_set() {
    assert!(Rss::now().expect("read").bytes() > 0);
}
