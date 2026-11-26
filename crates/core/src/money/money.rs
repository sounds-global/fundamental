#[cfg(not(feature = "std"))]
use alloc::format;
#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString};
use core::fmt;
use core::num::NonZeroU64;
use core::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::money::{Denomination, Division, Granularity, MoneyParseError, Rounding};

/// An exact amount of money, counted in millionths of a major unit.
///
/// It has no `Add`: a sum across denominations means nothing, so it belongs to a type that proved one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Money {
    micros: i64,
    denomination: Denomination,
}

impl Money {
    /// Millionths of a major unit per major unit.
    pub const MICROS_PER_MAJOR: i64 = 1_000_000;
    /// Millionths of a major unit per cent.
    pub const MICROS_PER_CENT: i64 = 10_000;

    /// An amount of `micros` millionths, in `denomination`.
    #[must_use]
    pub const fn from_micros(micros: i64, denomination: Denomination) -> Self {
        Self {
            micros,
            denomination,
        }
    }

    /// An amount of whole `cents`, in `denomination`.
    ///
    /// # Errors
    /// [`MoneyParseError::Overflow`] (`MONEY_004`) when `cents` does not fit in 64 bits of micros.
    pub fn from_cents(cents: i64, denomination: Denomination) -> Result<Self, MoneyParseError> {
        cents
            .checked_mul(Self::MICROS_PER_CENT)
            .map(|micros| Self::from_micros(micros, denomination))
            .ok_or_else(|| MoneyParseError::Overflow {
                amount: format!("{cents} cents"),
            })
    }

    /// The amount in whole cents; `None` when it is finer than a cent.
    #[must_use]
    pub const fn whole_cents(self) -> Option<i64> {
        if self.micros % Self::MICROS_PER_CENT == 0 {
            Some(self.micros / Self::MICROS_PER_CENT)
        } else {
            None
        }
    }

    /// Zero, in `denomination`.
    #[must_use]
    pub const fn zero(denomination: Denomination) -> Self {
        Self::from_micros(0, denomination)
    }

    /// The amount, in millionths of a major unit.
    #[must_use]
    pub const fn micros(self) -> i64 {
        self.micros
    }

    /// What this amount is counted in.
    #[must_use]
    pub const fn denomination(self) -> Denomination {
        self.denomination
    }

    /// This amount multiplied by a whole count — a per-unit price over `n` units.
    ///
    /// Saturating: a wrapped total would be a negative cost that passes every margin check.
    #[must_use]
    pub const fn times(self, n: u32) -> Self {
        Self::from_micros(self.micros.saturating_mul(n as i64), self.denomination)
    }

    /// This amount multiplied by a whole count, or `None` if the product does not fit.
    #[must_use]
    pub const fn checked_times(self, n: u64) -> Option<Self> {
        if n > i64::MAX as u64 {
            return None;
        }
        match self.micros.checked_mul(n as i64) {
            Some(micros) => Some(Self::from_micros(micros, self.denomination)),
            None => None,
        }
    }

    /// This amount divided into `parts` equal shares of whole `unit`s, the remainder handed back.
    ///
    /// # Returns
    ///
    /// `None` when a share, or a share moved one unit further from zero, would not fit in `i64` micros.
    #[must_use]
    pub const fn checked_div(self, parts: NonZeroU64, unit: Granularity) -> Option<Division> {
        let step = unit.micros_in(self.denomination);
        let divisor = parts.get() as i128 * step as i128;
        let units = self.micros as i128 / divisor;
        let remainder = self.micros as i128 % divisor;
        let each = units * step as i128;
        // The rounded figure is one step further from zero, so both must fit for `Division::rounded`.
        let furthest = if each < 0 {
            each - step as i128
        } else {
            each + step as i128
        };
        if furthest > i64::MAX as i128 || furthest < i64::MIN as i128 {
            return None;
        }
        Some(Division::of(
            Self::from_micros(each as i64, self.denomination),
            Self::from_micros(remainder as i64, self.denomination),
            parts,
            unit,
        ))
    }

    /// This amount resolved to whole `unit`s, `rounding` deciding which way.
    #[must_use]
    pub const fn rounded_to(self, unit: Granularity, rounding: Rounding) -> Self {
        let step = unit.micros_in(self.denomination);
        let remainder = self.micros % step;
        if remainder == 0 {
            return self;
        }
        let truncated = self.micros - remainder;
        if !rounding.carries(remainder.abs(), step) {
            return Self::from_micros(truncated, self.denomination);
        }
        let carried = if self.micros < 0 {
            truncated.saturating_sub(step)
        } else {
            truncated.saturating_add(step)
        };
        Self::from_micros(carried, self.denomination)
    }

    /// Whether this amount is strictly less than `other`.
    ///
    /// `None` across denominations, which is why this is not `PartialOrd`.
    #[must_use]
    pub fn is_less_than(self, other: Self) -> Option<bool> {
        (self.denomination == other.denomination).then_some(self.micros < other.micros)
    }
}

impl fmt::Display for Money {
    /// `€99.00`, `$266.42`, `€0.000037`, `-€1.70`: at least the minor digits, more when needed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.micros < 0 { "-" } else { "" };
        let magnitude = self.micros.unsigned_abs();
        let major = magnitude / Self::MICROS_PER_MAJOR.unsigned_abs();
        let fraction = magnitude % Self::MICROS_PER_MAJOR.unsigned_abs();

        let mut digits = format!("{fraction:06}");
        let floor = self.denomination.minor_digits() as usize;
        while digits.len() > floor && digits.ends_with('0') {
            digits.pop();
        }

        write!(f, "{sign}{}{major}.{digits}", self.denomination.symbol())
    }
}

impl FromStr for Money {
    type Err = MoneyParseError;

    /// `"EUR 99.00"`, `"USD 0.006"`, `"EUR -1.70"`.
    ///
    /// # Errors
    ///
    /// A [`MoneyParseError`] for a bad code or shape, a fraction finer than a millionth, or an overflow.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (code, amount) =
            s.trim()
                .split_once(' ')
                .ok_or_else(|| MoneyParseError::NotDecimal {
                    input: s.to_string(),
                })?;
        let denomination = match code.trim().to_ascii_uppercase().as_str() {
            "EUR" => Denomination::Eur,
            "USD" => Denomination::Usd,
            other => {
                return Err(MoneyParseError::UnknownDenomination {
                    code: other.to_string(),
                });
            }
        };

        let amount = amount.trim();
        let (negative, digits) = match amount.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, amount.strip_prefix('+').unwrap_or(amount)),
        };
        let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
        if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
            return Err(MoneyParseError::NotDecimal {
                input: s.to_string(),
            });
        }
        if !fraction.bytes().all(|b| b.is_ascii_digit()) {
            return Err(MoneyParseError::NotDecimal {
                input: s.to_string(),
            });
        }
        if fraction.len() > 6 {
            return Err(MoneyParseError::FinerThanAMicro {
                amount: amount.to_string(),
            });
        }

        let whole: i64 = whole.parse().map_err(|_| MoneyParseError::Overflow {
            amount: amount.to_string(),
        })?;
        let padded = format!("{fraction:0<6}");
        let fraction: i64 = padded.parse().map_err(|_| MoneyParseError::Overflow {
            amount: amount.to_string(),
        })?;
        let micros = whole
            .checked_mul(Self::MICROS_PER_MAJOR)
            .and_then(|m| m.checked_add(fraction))
            .ok_or_else(|| MoneyParseError::Overflow {
                amount: amount.to_string(),
            })?;

        Ok(Self::from_micros(
            if negative { -micros } else { micros },
            denomination,
        ))
    }
}

impl Serialize for Money {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let rendered = self.to_string();
        let without_symbol = rendered.replacen(self.denomination.symbol(), "", 1);
        serializer.serialize_str(&format!("{} {without_symbol}", self.denomination.code()))
    }
}

impl<'de> Deserialize<'de> for Money {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cents(amount: i64, denomination: Denomination) -> Money {
        Money::from_cents(amount, denomination).expect("a test amount fits")
    }

    #[test]
    fn whole_cents_are_ten_thousand_micros() {
        assert_eq!(cents(9900, Denomination::Eur).micros(), 99_000_000);
        assert_eq!(cents(-170, Denomination::Eur).micros(), -1_700_000);
    }

    #[test]
    fn cents_past_what_micros_hold_are_refused_rather_than_saturated() {
        let largest = i64::MAX / Money::MICROS_PER_CENT;
        assert!(Money::from_cents(largest, Denomination::Eur).is_ok());
        for amount in [largest + 1, i64::MAX, i64::MIN] {
            let refused = Money::from_cents(amount, Denomination::Eur)
                .expect_err("an amount past i64 micros is refused");
            assert!(matches!(refused, MoneyParseError::Overflow { .. }));
            assert_eq!(refused.code(), "MONEY_004");
        }
    }

    #[test]
    fn an_amount_of_whole_cents_gives_them_back() {
        for amount in [0, 1, 9900, -170, i64::MAX / Money::MICROS_PER_CENT] {
            assert_eq!(cents(amount, Denomination::Eur).whole_cents(), Some(amount));
        }
    }

    #[test]
    fn an_amount_finer_than_a_cent_has_no_whole_cents() {
        for micros in [1, 443_100, -5_000, 99_999_999, i64::MAX, i64::MIN] {
            assert_eq!(
                Money::from_micros(micros, Denomination::Eur).whole_cents(),
                None,
                "{micros}"
            );
        }
    }

    #[test]
    fn it_displays_a_catalogue_price_the_way_the_vendor_prints_it() {
        assert_eq!(cents(9900, Denomination::Eur).to_string(), "€99.00");
        assert_eq!(cents(26_642, Denomination::Usd).to_string(), "$266.42");
        assert_eq!(cents(50, Denomination::Eur).to_string(), "€0.50");
        assert_eq!(cents(4149, Denomination::Eur).to_string(), "€41.49");
    }

    #[test]
    fn it_holds_the_prices_cents_cannot() {
        assert_eq!(
            Money::from_micros(443_100, Denomination::Eur).to_string(),
            "€0.4431"
        );
        assert_eq!(
            Money::from_micros(6_000, Denomination::Usd).to_string(),
            "$0.006"
        );
        assert_eq!(
            Money::from_micros(2_500, Denomination::Eur).to_string(),
            "€0.0025"
        );
        let marginal = Money::from_micros(37, Denomination::Eur);
        assert_eq!(marginal.to_string(), "€0.000037");
        assert_ne!(marginal, Money::zero(Denomination::Eur), "not zero");
    }

    #[test]
    fn a_negative_amount_keeps_its_sign_in_front() {
        assert_eq!(cents(-170, Denomination::Eur).to_string(), "-€1.70");
    }

    #[test]
    fn a_nine_figure_count_is_checked_not_saturated() {
        let band = Money::from_micros(2_500, Denomination::Eur);
        assert_eq!(
            band.checked_times(100_000_000).map(Money::micros),
            Some(250_000_000_000)
        );
        assert_eq!(
            Money::from_micros(i64::MAX, Denomination::Eur).checked_times(2),
            None
        );
        assert_eq!(band.checked_times(u64::MAX), None);
    }

    #[test]
    fn a_refusal_names_its_reason() {
        assert!(matches!(
            "GBP 1.00".parse::<Money>(),
            Err(MoneyParseError::UnknownDenomination { .. })
        ));
        assert!(matches!(
            "EUR 0.0000001".parse::<Money>(),
            Err(MoneyParseError::FinerThanAMicro { .. })
        ));
        assert!(matches!(
            "EUR 99999999999999999".parse::<Money>(),
            Err(MoneyParseError::Overflow { .. })
        ));
        assert!(matches!(
            "99.00".parse::<Money>(),
            Err(MoneyParseError::NotDecimal { .. })
        ));
    }

    #[test]
    fn it_multiplies_by_a_count() {
        let ccx43_per_hour = Money::from_micros(443_100, Denomination::Eur);
        let three_domains = ccx43_per_hour.times(3);
        assert_eq!(three_domains.micros(), 1_329_300);
        assert_eq!(three_domains.to_string(), "€1.3293");
    }

    #[test]
    fn a_comparison_across_denominations_has_no_answer() {
        let eur = cents(9900, Denomination::Eur);
        let usd = cents(9900, Denomination::Usd);
        assert_eq!(eur.is_less_than(usd), None);
        assert_eq!(
            eur.is_less_than(cents(29_872, Denomination::Eur)),
            Some(true)
        );
    }

    #[test]
    fn cents_and_micros_agree() {
        assert_eq!(
            cents(9900, Denomination::Eur),
            Money::from_micros(99_000_000, Denomination::Eur)
        );
    }

    #[test]
    fn it_is_written_the_way_the_vendor_prints_it() {
        let ax42 = cents(9900, Denomination::Eur);
        let text = serde_json::to_string(&ax42).expect("serialises");
        assert_eq!(text, "\"EUR 99.00\"");
        let back: Money = serde_json::from_str(&text).expect("parses");
        assert_eq!(back, ax42);
    }

    #[test]
    fn a_sub_cent_price_parses_exactly() {
        assert_eq!(
            "EUR 0.4431".parse::<Money>().expect("parses"),
            Money::from_micros(443_100, Denomination::Eur)
        );
        assert_eq!(
            "USD 0.006".parse::<Money>().expect("parses"),
            Money::from_micros(6_000, Denomination::Usd)
        );
        assert_eq!(
            "EUR 0.0025".parse::<Money>().expect("parses"),
            Money::from_micros(2_500, Denomination::Eur)
        );
        assert_eq!(
            "EUR 0.000037".parse::<Money>().expect("parses"),
            Money::from_micros(37, Denomination::Eur)
        );
    }

    #[test]
    fn a_negative_amount_round_trips() {
        let credit = cents(-170, Denomination::Eur);
        let text = serde_json::to_string(&credit).expect("serialises");
        assert_eq!(text, "\"EUR -1.70\"");
        assert_eq!(
            serde_json::from_str::<Money>(&text).expect("parses"),
            credit
        );
    }

    #[test]
    fn it_refuses_what_it_cannot_hold_exactly() {
        for bad in [
            "EUR 0.0000001",
            "GBP 1.00",
            "99.00",
            "EUR abc",
            "EUR 1.2.3",
            "EUR ",
        ] {
            assert!(bad.parse::<Money>().is_err(), "'{bad}' must not parse");
        }
    }
}
