//! [`Money::typed`] — an amount as a person types it, in the one denomination they type in.

#[cfg(not(feature = "std"))]
use alloc::format;
#[cfg(not(feature = "std"))]
use alloc::string::ToString;

use crate::money::{Denomination, Money, MoneyParseError};

impl Money {
    /// The amount a person typed in `denomination`: `2200`, `2200.5`, `€ 2200,50` or
    /// `EUR 2200.50`, a minus in front of the figure for a negative one.
    ///
    /// A comma or point is the one decimal separator; `2,200` is refused rather than guessed at.
    ///
    /// # Errors
    ///
    /// [`MoneyParseError::NotDecimal`], [`MoneyParseError::OtherDenomination`],
    /// [`MoneyParseError::FinerThanItsMinorUnit`] or [`MoneyParseError::Overflow`].
    pub fn typed(text: &str, denomination: Denomination) -> Result<Self, MoneyParseError> {
        let figure = without_denomination(text.trim(), denomination)?;
        let (negative, digits) = figure
            .strip_prefix('-')
            .map_or((false, figure), |rest| (true, rest.trim_start()));
        let (whole, fraction) = digits.split_once(['.', ',']).unwrap_or((digits, ""));
        let decimal = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
        let separated = whole.len() < digits.len();
        if whole.is_empty()
            || !decimal(whole)
            || !decimal(fraction)
            || separated && fraction.is_empty()
        {
            return Err(MoneyParseError::NotDecimal {
                input: text.to_string(),
            });
        }
        if fraction.len() > denomination.minor_digits() as usize {
            return Err(MoneyParseError::FinerThanItsMinorUnit {
                amount: figure.to_string(),
            });
        }
        let sign = if negative { "-" } else { "" };
        format!("{} {sign}{whole}.{fraction}", denomination.code()).parse()
    }
}

/// `text` without the code or symbol of `denomination` in front; another denomination's refused.
fn without_denomination(text: &str, denomination: Denomination) -> Result<&str, MoneyParseError> {
    for own in [denomination.symbol(), denomination.code()] {
        if let Some(rest) = strip_prefix_ignoring_case(text, own) {
            return Ok(rest.trim_start());
        }
    }
    let other = Denomination::ALL.into_iter().find(|other| {
        [other.symbol(), other.code()]
            .into_iter()
            .any(|written| strip_prefix_ignoring_case(text, written).is_some())
    });
    match other {
        Some(other) => Err(MoneyParseError::OtherDenomination {
            code: other.code().to_string(),
        }),
        None => Ok(text),
    }
}

fn strip_prefix_ignoring_case<'t>(text: &'t str, prefix: &str) -> Option<&'t str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn euros(text: &str) -> Result<Money, MoneyParseError> {
        Money::typed(text, Denomination::Eur)
    }

    fn cents(cents: i64) -> Money {
        Money::from_cents(cents, Denomination::Eur).expect("cents that fit")
    }

    #[test]
    fn an_amount_is_read_as_a_person_types_it() {
        assert_eq!(euros("2200"), Ok(cents(220_000)));
        assert_eq!(euros("2200.5"), Ok(cents(220_050)));
        assert_eq!(euros("2200,50"), Ok(cents(220_050)));
        assert_eq!(euros("€ 2200.00"), Ok(cents(220_000)));
        assert_eq!(euros("€2200"), Ok(cents(220_000)));
        assert_eq!(euros(" EUR 0.07 "), Ok(cents(7)));
        assert_eq!(euros("eur 12"), Ok(cents(1_200)));
        assert_eq!(euros("-1.70"), Ok(cents(-170)));
        assert_eq!(euros("€ -1,70"), Ok(cents(-170)));
    }

    #[test]
    fn what_is_no_figure_is_refused() {
        for text in [
            "", "€ ", "EUR", "twee", "£12", "2200.", ".50", "1.2.3", "2 200", "--1", "1e3",
        ] {
            assert!(
                matches!(euros(text), Err(MoneyParseError::NotDecimal { .. })),
                "{text:?}: {:?}",
                euros(text)
            );
        }
    }

    #[test]
    fn a_grouped_figure_or_one_finer_than_a_cent_is_refused() {
        for text in ["2,200", "2.200", "0.001", "1,505"] {
            assert!(
                matches!(
                    euros(text),
                    Err(MoneyParseError::FinerThanItsMinorUnit { .. })
                ),
                "{text:?}: {:?}",
                euros(text)
            );
        }
    }

    #[test]
    fn an_amount_in_another_denomination_is_refused() {
        for text in ["$ 12", "USD 12", "usd12"] {
            assert!(
                matches!(euros(text), Err(MoneyParseError::OtherDenomination { .. })),
                "{text:?}: {:?}",
                euros(text)
            );
        }
        assert_eq!(
            Money::typed("$ 12", Denomination::Usd),
            Money::from_cents(1_200, Denomination::Usd)
        );
    }

    #[test]
    fn an_amount_too_large_to_hold_is_refused() {
        assert!(matches!(
            euros("99999999999999999999"),
            Err(MoneyParseError::Overflow { .. })
        ));
    }
}
