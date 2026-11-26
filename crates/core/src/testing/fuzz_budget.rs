use crate::env::EnvVars;
use crate::testing::FuzzBudgetError;
use std::time::Duration;

/// How long a property runs, and the seed every batch is derived from.
///
/// Unset, it runs a fixed seed and case count; `FUNDAMENTAL_FUZZ_FOR` and `FUNDAMENTAL_FUZZ_SEED` override.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuzzBudget {
    until: Until,
    root_seed: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Until {
    Cases(u32),
    Elapsed(Duration),
}

const DEFAULT_CASES: u32 = 512;

const DEFAULT_SEED: [u8; 32] = [0x5A; 32];

impl FuzzBudget {
    /// A fixed number of cases from the default seed.
    #[must_use]
    pub const fn cases(cases: u32) -> Self {
        Self {
            until: Until::Cases(cases),
            root_seed: DEFAULT_SEED,
        }
    }

    /// Run until `elapsed` has passed, from `root_seed`.
    #[must_use]
    pub const fn for_duration(elapsed: Duration, root_seed: [u8; 32]) -> Self {
        Self {
            until: Until::Elapsed(elapsed),
            root_seed,
        }
    }

    /// The budget `FUNDAMENTAL_FUZZ_FOR` and `FUNDAMENTAL_FUZZ_SEED` describe.
    ///
    /// # Errors
    ///
    /// [`FuzzBudgetError`] when either is set to something other than `<seconds>s` or 64 hex characters.
    pub fn from_env(env: &EnvVars) -> Result<Self, FuzzBudgetError> {
        let root_seed = match env.get("FUNDAMENTAL_FUZZ_SEED") {
            Some(text) => parse_seed(&text)?,
            None => DEFAULT_SEED,
        };
        let until = match env.get("FUNDAMENTAL_FUZZ_FOR") {
            Some(text) => Until::Elapsed(parse_duration(&text)?),
            None => Until::Cases(DEFAULT_CASES),
        };
        Ok(Self { until, root_seed })
    }

    /// The seed every batch derives from.
    #[must_use]
    pub const fn root_seed(&self) -> &[u8; 32] {
        &self.root_seed
    }

    /// The deadline, when this budget runs for a duration.
    #[must_use]
    pub const fn duration(&self) -> Option<Duration> {
        match self.until {
            Until::Elapsed(elapsed) => Some(elapsed),
            Until::Cases(_) => None,
        }
    }

    /// The case count, when this budget runs a fixed number.
    #[must_use]
    pub const fn fixed_cases(&self) -> Option<u32> {
        match self.until {
            Until::Cases(cases) => Some(cases),
            Until::Elapsed(_) => None,
        }
    }
}

fn parse_duration(text: &str) -> Result<Duration, FuzzBudgetError> {
    text.strip_suffix('s')
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|digits| digits.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .ok_or_else(|| FuzzBudgetError::Duration(text.to_owned()))
}

fn parse_seed(text: &str) -> Result<[u8; 32], FuzzBudgetError> {
    let malformed = || FuzzBudgetError::Seed(text.to_owned());
    if text.len() != 64 {
        return Err(malformed());
    }
    let mut seed = [0_u8; 32];
    for (byte, pair) in seed.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        let pair = core::str::from_utf8(pair).map_err(|_| malformed())?;
        *byte = u8::from_str_radix(pair, 16).map_err(|_| malformed())?;
    }
    Ok(seed)
}

#[cfg(test)]
mod tests {
    use super::FuzzBudget;
    use crate::env::EnvVars;
    use crate::testing::FuzzBudgetError;
    use std::time::Duration;

    #[test]
    fn no_setting_is_the_fixed_deterministic_default() {
        let budget = FuzzBudget::from_env(&EnvVars::none()).expect("nothing set");
        assert_eq!(budget, FuzzBudget::cases(512));
    }

    #[test]
    fn a_duration_and_a_seed_are_read() {
        let seed = "00".repeat(31) + "ff";
        let env = EnvVars::fixed([
            ("FUNDAMENTAL_FUZZ_FOR", "3600s"),
            ("FUNDAMENTAL_FUZZ_SEED", seed.as_str()),
        ]);
        let budget = FuzzBudget::from_env(&env).expect("both well formed");
        assert_eq!(budget.duration(), Some(Duration::from_secs(3_600)));
        assert_eq!(budget.root_seed()[31], 0xFF);
    }

    #[test]
    fn a_mistyped_budget_is_refused_not_defaulted() {
        for text in ["3600", "1h", "0s", "s", "-5s", " 60s"] {
            let env = EnvVars::fixed([("FUNDAMENTAL_FUZZ_FOR", text)]);
            assert!(
                matches!(
                    FuzzBudget::from_env(&env),
                    Err(FuzzBudgetError::Duration(_))
                ),
                "{text} was accepted"
            );
        }
        for text in ["00", &"zz".repeat(32), &"0".repeat(65)] {
            let env = EnvVars::fixed([("FUNDAMENTAL_FUZZ_SEED", text)]);
            assert!(matches!(
                FuzzBudget::from_env(&env),
                Err(FuzzBudgetError::Seed(_))
            ));
        }
    }
}
