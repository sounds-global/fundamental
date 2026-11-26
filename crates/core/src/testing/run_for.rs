use crate::testing::{FuzzBudget, FuzzFailure, FuzzRecord};
use core::fmt::Debug;
use proptest::prelude::RngCore;
use proptest::strategy::Strategy;
use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestError, TestRng, TestRunner};
use std::time::Instant;

const BATCH_CASES: u32 = 256;

/// Run `property` over `strategy` until `budget` is spent, one seeded batch at a time.
///
/// Batch seeds come from one ChaCha stream keyed by the root seed, so a run replays exactly.
///
/// # Errors
///
/// [`FuzzFailure`] with the shrunk input and its batch, the first time the property fails.
pub fn run_for<S>(
    target: &str,
    budget: &FuzzBudget,
    strategy: S,
    property: impl Fn(S::Value) -> Result<(), TestCaseError>,
) -> Result<FuzzRecord, FuzzFailure>
where
    S: Strategy,
    S::Value: Debug,
{
    let started = Instant::now();
    let root_seed = hex(budget.root_seed());
    let mut seeds = TestRng::from_seed(RngAlgorithm::ChaCha, budget.root_seed());
    let batch_cases = budget.fixed_cases().unwrap_or(BATCH_CASES);
    let mut cases = 0_u64;
    let mut batches = 0_u64;
    loop {
        let mut batch_seed = [0_u8; 32];
        seeds.fill_bytes(&mut batch_seed);
        let config = Config {
            cases: batch_cases,
            failure_persistence: None,
            ..Config::default()
        };
        let mut runner = TestRunner::new_with_rng(
            config,
            TestRng::from_seed(RngAlgorithm::ChaCha, &batch_seed),
        );
        let outcome = runner.run(&strategy, &property);
        if let Err(error) = outcome {
            let (reason, input) = match error {
                TestError::Fail(reason, input) => (reason.to_string(), format!("{input:?}")),
                TestError::Abort(reason) => (reason.to_string(), String::from("(none)")),
            };
            return Err(FuzzFailure {
                target: target.to_owned(),
                root_seed,
                batch: batches,
                reason,
                input,
            });
        }
        cases += u64::from(batch_cases);
        batches += 1;
        let spent = match budget.duration() {
            Some(deadline) => started.elapsed() >= deadline,
            None => true,
        };
        if spent {
            return Ok(FuzzRecord {
                target: target.to_owned(),
                cases,
                batches,
                root_seed,
                elapsed: started.elapsed(),
            });
        }
    }
}

fn hex(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0xF)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::run_for;
    use crate::testing::FuzzBudget;
    use proptest::prelude::*;
    use std::time::Duration;

    #[test]
    fn a_fixed_budget_runs_its_cases_once() {
        let record = run_for("always", &FuzzBudget::cases(64), any::<u8>(), |_| Ok(()))
            .expect("a property that always holds");
        assert_eq!((record.cases, record.batches), (64, 1));
        assert!(
            record
                .to_string()
                .starts_with("fuzz-record target=always cases=64 batches=1 root_seed=5a5a")
        );
    }

    #[test]
    fn a_duration_runs_batches_until_the_deadline() {
        let budget = FuzzBudget::for_duration(Duration::from_millis(50), [1; 32]);
        let record = run_for("always", &budget, any::<u8>(), |_| Ok(())).expect("holds");
        assert!(record.batches >= 1);
        assert_eq!(record.cases, record.batches * 256);
        assert!(record.elapsed >= Duration::from_millis(50));
    }

    #[test]
    fn a_failure_is_shrunk_and_names_its_seed() {
        let failure = run_for(
            "below_200",
            &FuzzBudget::cases(4_096),
            any::<u8>(),
            |value| {
                prop_assert!(value < 200);
                Ok(())
            },
        )
        .expect_err("some byte is at least 200");
        assert_eq!(failure.input, "200");
        assert_eq!(failure.root_seed, "5a".repeat(32));
    }

    #[test]
    fn a_panic_is_a_failure() {
        let failure = run_for(
            "panics",
            &FuzzBudget::cases(16),
            any::<u8>(),
            |_| -> Result<(), TestCaseError> { panic!("the property panicked") },
        )
        .expect_err("a panic fails the property");
        assert!(
            failure.reason.contains("the property panicked"),
            "{}",
            failure.reason
        );
    }

    #[test]
    fn the_same_seed_finds_the_same_failure() {
        let run = || {
            run_for("rare", &FuzzBudget::cases(4_096), any::<u32>(), |value| {
                prop_assert!(value % 1_000 != 7);
                Ok(())
            })
        };
        assert_eq!(run().expect_err("found"), run().expect_err("found"));
    }
}
