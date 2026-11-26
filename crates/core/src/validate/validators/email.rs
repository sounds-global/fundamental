//! Email validation.

use crate::MailAddress;
use crate::validate::ValidationError;

pub fn validate_email(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Ok(());
    }
    MailAddress::parse(value.trim_matches([' ', '\t']))
        .map(|_| ())
        .map_err(|problem| {
            ValidationError::invalid_email(field).with_param("reason", problem.code())
        })
}

#[cfg(test)]
mod tests {
    use super::validate_email;
    use crate::MailAddress;

    #[cfg(not(feature = "std"))]
    use alloc::format;

    #[test]
    fn validate_email_agrees_with_mail_address() {
        for value in [
            "test@example.com",
            "user.name@domain.co.uk",
            "invalid",
            "@domain.com",
            "user@",
            "eve@evil.nl>, victim@bank.nl",
            "a b@example.nl",
            "jürgen@example.de",
            "a..b@example.nl",
        ] {
            assert_eq!(
                validate_email("email", value).is_ok(),
                MailAddress::parse(value.trim()).is_ok(),
                "{value:?}"
            );
        }
    }

    #[test]
    fn a_refusal_names_its_rule_and_not_the_value() {
        let error = validate_email("email", "eve@evil.nl>, victim@bank.nl").expect_err("refused");
        assert_eq!(error.code, "invalid_email");
        let shown = format!("{error:?}");
        assert!(shown.contains("address_forbidden_character"), "{shown}");
        assert!(!shown.contains("victim"), "{shown}");
    }

    #[test]
    fn the_padding_around_a_pasted_address_is_set_aside() {
        assert!(validate_email("email", "  J.Jansen@Firma.NL ").is_ok());
        assert!(validate_email("email", "\tj@firma.nl").is_ok());
        assert!(validate_email("email", " a b@firma.nl ").is_err());
        assert!(validate_email("email", "j@firma.nl\r\n").is_err());
    }

    #[test]
    fn an_empty_value_is_left_to_required() {
        assert!(validate_email("email", "").is_ok());
    }
}
