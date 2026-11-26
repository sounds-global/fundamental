//! [`Refusal`] — a refused request's code and the numbers its sentence needs.

use std::borrow::Cow;

use super::Error;
use crate::address::{PagePath, ReturnPath};
use crate::i18n::{ArgValue, translate};

/// The query parameter that names the refused code.
const CODE_PARAM: &str = "refused";

/// The prefix of each number's query parameter: `n_attempts_left=2`.
const NUMBER_PREFIX: &str = "n_";

/// The most numbers one refusal carries; a message needs one or two.
const MAX_NUMBERS: usize = 8;

/// The longest code or argument name a refusal accepts.
const MAX_WORD: usize = 64;

/// A refusal on its way back to the page whose form it refused: its code and whole numbers.
///
/// Only whole numbers travel: an address is logged and shared, and a string may be personal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    code: String,
    numbers: Vec<(String, i64)>,
}

impl Refusal {
    /// The refusal `error` makes: its code and its message's whole numbers.
    #[must_use]
    pub fn of(error: &dyn Error) -> Self {
        Self {
            code: error.error_code().to_owned(),
            numbers: error
                .message_numbers()
                .into_iter()
                .filter(|(name, _)| is_word(name))
                .take(MAX_NUMBERS)
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
        }
    }

    /// The refused code, such as `ACCT_004`.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The numbers the refusal's sentence needs, by argument name.
    #[must_use]
    pub fn numbers(&self) -> &[(String, i64)] {
        &self.numbers
    }

    /// The refusal as query parameters, without a leading `?` or `&`: `refused=ACCT_004&n_attempts_left=2`.
    #[must_use]
    pub fn query(&self) -> String {
        let mut query = format!("{CODE_PARAM}={}", self.code);
        for (name, value) in &self.numbers {
            query.push_str(&format!("&{NUMBER_PREFIX}{name}={value}"));
        }
        query
    }

    /// The refusal `query` carries, if any; a code that is not a word refuses the whole refusal.
    #[must_use]
    pub fn from_query(query: &str) -> Option<Self> {
        let mut code = None;
        let mut numbers = Vec::new();
        for pair in query.trim_start_matches('?').split('&') {
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            if name == CODE_PARAM {
                code = Some(value);
            } else if let Some(argument) = name.strip_prefix(NUMBER_PREFIX)
                && is_word(argument)
                && numbers.len() < MAX_NUMBERS
                && let Some(value) = parse_number(value)
            {
                numbers.push((argument.to_owned(), value));
            }
        }
        let code = code.filter(|code| is_word(code))?;
        Some(Self {
            code: code.to_owned(),
            numbers,
        })
    }

    /// `path` without any refusal it carries.
    #[must_use]
    pub fn cleared_from(path: ReturnPath) -> ReturnPath {
        path.without_query_params(|key| key == CODE_PARAM || key.starts_with(NUMBER_PREFIX))
    }

    /// The refusal's sentence in the current language, or `fallback` when it cannot be said.
    pub fn message<'a>(&self, fallback: impl FnOnce() -> Cow<'a, str>) -> Cow<'a, str> {
        let key = fundamental_i18n_keys::error_key(&self.code, None);
        let args: Vec<(&str, ArgValue<'_>)> = self
            .numbers
            .iter()
            .map(|(name, value)| (name.as_str(), ArgValue::Int(*value)))
            .collect();
        translate(&key, &args, fallback)
    }
}

/// Whether `text` is a code or an argument name: ASCII letters, digits and `_`, bounded.
fn is_word(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_WORD
        && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// `text` as a whole number written in decimal digits, with an optional leading `-`.
fn parse_number(text: &str) -> Option<i64> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || digits.len() > 18 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

impl PagePath {
    /// The same page carrying `refusal` back to it in the pairs [`Refusal::query`] writes.
    #[must_use]
    pub fn with_refusal(self, refusal: &Refusal) -> Self {
        let page = self.with_query(CODE_PARAM, &refusal.code);
        refusal.numbers.iter().fold(page, |page, (name, value)| {
            page.with_query(&format!("{NUMBER_PREFIX}{name}"), &value.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;

    #[derive(Debug)]
    struct WrongCode(u32);

    impl fmt::Display for WrongCode {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("wrong code")
        }
    }

    impl core::error::Error for WrongCode {}

    impl Error for WrongCode {
        fn error_code(&self) -> &'static str {
            "TEST_004"
        }

        fn display_message(&self) -> Cow<'_, str> {
            Cow::Owned(format!("You have {} attempts left.", self.0))
        }

        fn message_numbers(&self) -> Vec<(&'static str, i64)> {
            vec![("attempts_left", i64::from(self.0))]
        }
    }

    #[test]
    fn a_refusal_crosses_an_address_whole() {
        let refusal = Refusal::of(&WrongCode(2));
        assert_eq!(refusal.query(), "refused=TEST_004&n_attempts_left=2");
        let back = Refusal::from_query(&format!("?next=%2Fx&{}", refusal.query()));
        assert_eq!(back, Some(refusal));
    }

    #[test]
    fn a_page_carries_a_refusal_back() {
        let refusal = Refusal::of(&WrongCode(2));
        let page = PagePath::of_route("/sign-in/code").with_refusal(&refusal);
        let spelled = crate::address::AddressSpace::default().page_address("", &page);
        assert_eq!(spelled, "/sign-in/code?refused=TEST_004&n_attempts_left=2");
        let query = spelled.split_once('?').map(|(_, query)| query);
        assert_eq!(
            Refusal::from_query(query.unwrap_or_default()),
            Some(refusal)
        );
    }

    #[test]
    fn an_address_without_a_word_for_a_code_carries_no_refusal() {
        assert_eq!(Refusal::from_query("done=invited"), None);
        assert_eq!(Refusal::from_query("refused=%3Cscript%3E"), None);
        assert_eq!(Refusal::from_query("refused="), None);
    }

    #[test]
    fn only_numbers_come_through_and_at_most_eight() {
        let refusal =
            Refusal::from_query("refused=X_1&n_a=1&n_b=Jan&n_c=1e9&n_d=-3&n_%3C=4").unwrap();
        assert_eq!(
            refusal.numbers(),
            [("a".to_owned(), 1), ("d".to_owned(), -3)]
        );
        let many: String = (0..20).map(|i| format!("&n_k{i}={i}")).collect();
        let refusal = Refusal::from_query(&format!("refused=X_1{many}")).unwrap();
        assert_eq!(refusal.numbers().len(), MAX_NUMBERS);
    }

    #[test]
    fn a_number_past_i64_is_left_out() {
        let refusal = Refusal::from_query("refused=X_1&n_a=99999999999999999999").unwrap();
        assert!(refusal.numbers().is_empty());
    }

    #[test]
    fn a_new_refusal_replaces_the_one_an_address_already_carries() {
        let back = ReturnPath::parse("/sign-in/code?next=%2Fx&refused=TEST_004&n_attempts_left=2")
            .expect("a local path");
        assert_eq!(
            Refusal::cleared_from(back).as_location(),
            "/sign-in/code?next=%2Fx"
        );
    }

    #[test]
    fn without_a_catalog_the_message_is_the_fallback() {
        let refusal = Refusal::of(&WrongCode(2));
        assert_eq!(
            refusal.message(|| Cow::Borrowed("Try again.")),
            "Try again."
        );
    }
}
