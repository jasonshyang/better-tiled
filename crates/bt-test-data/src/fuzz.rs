//! Randomised tests: their seed and number of cases come from the environment, so a failure
//! reproduces with one command. `BT_FUZZ_SEED` sets the seed and `BT_FUZZ_CASES` the count; a
//! test that fails prints both with the case it generated.

/// The environment variable that sets a randomised test's seed.
pub const FUZZ_SEED_VAR: &str = "BT_FUZZ_SEED";

/// The environment variable that sets how many cases a randomised test runs.
pub const FUZZ_CASES_VAR: &str = "BT_FUZZ_CASES";

/// What a randomised test runs: the seed its cases derive from, and how many there are.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Fuzz {
    /// The seed.
    pub seed: u64,
    /// How many cases.
    pub cases: u32,
}

impl Fuzz {
    /// The seed and count from the environment, or the test's own when unset. A value that is
    /// not a number fails the test rather than running something else.
    #[allow(
        clippy::panic,
        reason = "a mistyped variable must fail the calling test, not run other cases"
    )]
    pub fn from_env(default_seed: u64, default_cases: u32) -> Fuzz {
        let read = |var: &str| -> Option<u64> {
            let value = std::env::var(var).ok()?;
            match value.trim().parse() {
                Ok(number) => Some(number),
                Err(_) => panic!("{var}={value} is not a whole number"),
            }
        };
        Fuzz {
            seed: read(FUZZ_SEED_VAR).unwrap_or(default_seed),
            cases: read(FUZZ_CASES_VAR)
                .map_or(default_cases, |n| u32::try_from(n).unwrap_or(u32::MAX)),
        }
    }

    /// The seed of case `case`: each case its own, so one can be run again alone.
    pub fn case_seed(&self, case: u32) -> u64 {
        self.seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(u64::from(case))
    }

    /// The command-line prefix that runs these cases again.
    pub fn reproduce(&self) -> String {
        format!(
            "{FUZZ_SEED_VAR}={} {FUZZ_CASES_VAR}={}",
            self.seed, self.cases
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cases_have_seeds_of_their_own() {
        let fuzz = Fuzz { seed: 7, cases: 3 };
        assert_ne!(fuzz.case_seed(0), fuzz.case_seed(1));
        assert_eq!(fuzz.reproduce(), "BT_FUZZ_SEED=7 BT_FUZZ_CASES=3");
    }
}
