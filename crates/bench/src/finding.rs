use std::fmt;

/// One way a measured instruction count disagrees with the baseline.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Finding {
    /// The bench executes more instructions than the baseline allows.
    Regressed {
        /// The bench, as `<package>/<bench file>/<group>/<function>.<id>`.
        bench: String,
        /// Its count in the baseline.
        was: u64,
        /// Its count now.
        now: u64,
        /// The growth allowed, as a fraction (`0.02` is 2%).
        limit: f64,
    },
    /// The baseline names a bench that no longer ran.
    NotMeasured {
        /// The bench.
        bench: String,
    },
    /// A bench ran that the baseline does not name.
    NotInBaseline {
        /// The bench.
        bench: String,
    },
}

impl Finding {
    /// The bench it is about.
    #[must_use]
    pub fn bench(&self) -> &str {
        match self {
            Self::Regressed { bench, .. }
            | Self::NotMeasured { bench }
            | Self::NotInBaseline { bench } => bench,
        }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Regressed {
                bench,
                was,
                now,
                limit,
            } => {
                #[expect(clippy::cast_precision_loss, reason = "a percentage for a person")]
                let growth = (*now as f64 / *was.max(&1) as f64 - 1.0) * 100.0;
                write!(
                    f,
                    "{bench}: {was} -> {now} instructions (+{growth:.2}%, limit {:.2}%)",
                    limit * 100.0
                )
            }
            Self::NotMeasured { bench } => write!(f, "{bench}: in the baseline but not measured"),
            Self::NotInBaseline { bench } => write!(f, "{bench}: measured but not in the baseline"),
        }
    }
}
