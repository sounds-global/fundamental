use serde::{Deserialize, Serialize};

/// The unit a [`Money`](crate::money::Money) amount is counted in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Denomination {
    /// Euro. Two minor digits.
    Eur,
    /// United States dollar. Two minor digits.
    Usd,
}

impl Denomination {
    /// Every denomination, in the order they are declared.
    pub(crate) const ALL: [Self; 2] = [Self::Eur, Self::Usd];

    /// How many minor units make one major unit, as a power of ten.
    #[must_use]
    pub const fn minor_digits(self) -> u32 {
        match self {
            Self::Eur | Self::Usd => 2,
        }
    }

    /// The ISO 4217 alphabetic code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Eur => "EUR",
            Self::Usd => "USD",
        }
    }

    /// The denomination whose ISO 4217 alphabetic [`code`](Self::code) is exactly `code`.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|denomination| denomination.code() == code)
    }

    /// The symbol used when a figure is written for a human reader.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Eur => "€",
            Self::Usd => "$",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_and_symbols_are_the_ones_a_gate_greps_for() {
        assert_eq!(Denomination::Eur.code(), "EUR");
        assert_eq!(Denomination::Usd.code(), "USD");
        assert_eq!(Denomination::Eur.symbol(), "€");
        assert_eq!(Denomination::Usd.symbol(), "$");
    }

    #[test]
    fn every_variant_has_a_distinct_symbol() {
        let all = [Denomination::Eur, Denomination::Usd];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a.symbol(), b.symbol());
                assert_ne!(a.code(), b.code());
            }
        }
    }

    #[test]
    fn it_reads_back_from_its_code_and_nothing_else() {
        for denomination in [Denomination::Eur, Denomination::Usd] {
            assert_eq!(
                Denomination::from_code(denomination.code()),
                Some(denomination)
            );
        }
        for unknown in ["eur", "GBP", "", "EURO"] {
            assert_eq!(Denomination::from_code(unknown), None);
        }
    }

    #[test]
    fn it_round_trips_through_its_lower_case_wire_form() {
        let parsed: Denomination = serde_json::from_str("\"eur\"").expect("eur parses");
        assert_eq!(parsed, Denomination::Eur);
        assert_eq!(
            serde_json::to_string(&Denomination::Usd).expect("serialises"),
            "\"usd\""
        );
    }
}
