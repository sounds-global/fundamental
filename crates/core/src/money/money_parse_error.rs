//! [`MoneyParseError`] — why text is not an exact amount of money.

#[cfg(not(feature = "std"))]
use alloc::borrow::Cow;
#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString};
use core::fmt;
#[cfg(feature = "std")]
use std::borrow::Cow;

/// Why a string could not be read as a [`Money`](crate::money::Money).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MoneyParseError {
    /// The code before the amount names no known denomination.
    UnknownDenomination {
        /// The code as written.
        code: String,
    },
    /// The text is not `<CODE> <decimal amount>`.
    NotDecimal {
        /// The text as written.
        input: String,
    },
    /// The amount has more than six fractional digits.
    FinerThanAMicro {
        /// The amount as written.
        amount: String,
    },
    /// The amount does not fit in 64 bits of micros.
    Overflow {
        /// The amount as written.
        amount: String,
    },
    /// A typed amount has more decimals than its denomination's minor unit.
    FinerThanItsMinorUnit {
        /// The amount as written.
        amount: String,
    },
    /// A typed amount is written in another denomination than the one it is typed in.
    OtherDenomination {
        /// The other denomination's code.
        code: String,
    },
}

impl MoneyParseError {
    /// The stable code for this variant.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownDenomination { .. } => "MONEY_001",
            Self::NotDecimal { .. } => "MONEY_002",
            Self::FinerThanAMicro { .. } => "MONEY_003",
            Self::Overflow { .. } => "MONEY_004",
            Self::FinerThanItsMinorUnit { .. } => "MONEY_005",
            Self::OtherDenomination { .. } => "MONEY_006",
        }
    }
}

impl fmt::Display for MoneyParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownDenomination { code } => {
                write!(f, "'{code}' is not a known denomination")
            }
            Self::NotDecimal { input } => {
                write!(f, "expected '<CODE> <decimal amount>', got '{input}'")
            }
            Self::FinerThanAMicro { amount } => write!(
                f,
                "'{amount}' is finer than a millionth, which cannot be held exactly"
            ),
            Self::Overflow { amount } => write!(f, "'{amount}' does not fit"),
            Self::FinerThanItsMinorUnit { amount } => {
                write!(f, "'{amount}' has more decimals than its minor unit")
            }
            Self::OtherDenomination { code } => {
                write!(f, "an amount in {code} is not one in this denomination")
            }
        }
    }
}

impl core::error::Error for MoneyParseError {}

impl crate::Error for MoneyParseError {
    fn error_code(&self) -> &'static str {
        self.code()
    }

    fn display_message(&self) -> Cow<'_, str> {
        Cow::Owned(self.to_string())
    }

    fn http_status_code(&self) -> u16 {
        400
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_has_its_own_code() {
        let codes = [
            MoneyParseError::UnknownDenomination {
                code: String::new(),
            }
            .code(),
            MoneyParseError::NotDecimal {
                input: String::new(),
            }
            .code(),
            MoneyParseError::FinerThanAMicro {
                amount: String::new(),
            }
            .code(),
            MoneyParseError::Overflow {
                amount: String::new(),
            }
            .code(),
            MoneyParseError::FinerThanItsMinorUnit {
                amount: String::new(),
            }
            .code(),
            MoneyParseError::OtherDenomination {
                code: String::new(),
            }
            .code(),
        ];
        for (i, a) in codes.iter().enumerate() {
            for b in &codes[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }
}
