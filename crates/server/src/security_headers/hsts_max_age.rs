use serde::Deserialize;

/// How long a browser keeps a site HTTPS-only, as the steps of a ramp; only [`TwoYears`](Self::TwoYears) may be preloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HstsMaxAge {
    /// 300 seconds: the first step, while checking nothing breaks.
    FiveMinutes,
    /// One week.
    OneWeek,
    /// One month (30 days).
    OneMonth,
    /// One year (365 days).
    OneYear,
    /// Two years: the only step the preload list accepts.
    TwoYears,
}

impl HstsMaxAge {
    /// The step in seconds, as `max-age` states it.
    #[must_use]
    pub const fn seconds(self) -> u32 {
        match self {
            Self::FiveMinutes => 300,
            Self::OneWeek => 604_800,
            Self::OneMonth => 2_592_000,
            Self::OneYear => 31_536_000,
            Self::TwoYears => 63_072_000,
        }
    }
}
