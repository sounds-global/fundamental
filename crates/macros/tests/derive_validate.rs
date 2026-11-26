//! Integration tests for the Validate derive macro.
#![allow(missing_docs)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use fundamental::Validate;
use fundamental::core::validate::{ValidationError, ValidationErrors, ValidationResult};

#[derive(Debug, Validate)]
pub struct Address {
    #[validate(required)]
    pub street: String,

    #[validate(length(min = 2))]
    pub city: String,
}

// Returns `ValidationErrors` so the derive can merge them under the field's prefix.
fn validate_phone(value: &Option<String>) -> ValidationResult<()> {
    if let Some(v) = value
        && v.len() < 7
    {
        let mut errs = ValidationErrors::new();
        errs.add(ValidationError::custom(
            "invalid_phone",
            "phone must be at least 7 characters",
        ));
        return Err(errs);
    }
    Ok(())
}

fn validate_multi_str(value: &str) -> Result<(), ValidationErrors> {
    let mut errs = ValidationErrors::new();
    if value.is_empty() {
        errs.add(ValidationError::custom("empty_field", "cannot be empty"));
    }
    if value.len() < 3 {
        errs.add(ValidationError::custom(
            "too_short",
            "must be at least 3 chars",
        ));
    }

    errs.into_result()
}

#[derive(Debug, Validate)]
pub struct TestInput {
    #[validate(required)]
    pub name: String,

    #[validate(length(min = 3, max = 8))]
    pub short_name: String,

    #[validate(range(min = 1, max = 10))]
    pub count: u8,

    #[validate(email)]
    pub email: String,

    #[validate(url)]
    pub website: String,

    #[validate(uuid)]
    pub uuid: String,

    #[validate(alphanumeric)]
    pub alnum: String,

    #[validate(ascii)]
    pub ascii: String,

    #[validate(one_of("a", "b", "c"))]
    pub choice: String,

    #[validate(nested)]
    pub address: Address,

    #[validate(custom = "validate_phone")]
    pub phone: Option<String>,

    #[validate(skip)]
    pub skipped: String,
}

#[test]
fn test_valid_input_passes() {
    let input = TestInput {
        name: "John".into(),
        short_name: "Johnny".into(),
        count: 5,
        email: "user@example.com".into(),
        website: "https://example.com".into(),
        uuid: "550e8400-e29b-41d4-a716-446655440000".into(),
        alnum: "User123".into(),
        ascii: "ASCII_ONLY".into(),
        choice: "b".into(),
        address: Address {
            street: "12 Elm St".into(),
            city: "NY".into(),
        },
        phone: Some("1234567".into()),
        skipped: "☃".into(),
    };

    assert!(input.validate().is_ok());
}

#[test]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn test_invalid_input_collects_all_errors() {
    let input = TestInput {
        name: "".into(),                     // required
        short_name: "ab".into(),             // too short (min 3)
        count: 0,                            // too small (min 1)
        email: "invalid".into(),             // invalid email
        website: "ftp://example.com".into(), // invalid url
        uuid: "not-a-uuid".into(),           // invalid uuid
        alnum: "User!".into(),               // not alphanumeric
        ascii: "é".into(),                   // non-ascii
        choice: "x".into(),                  // not in allowed list
        address: Address {
            street: "".into(), // required
            city: "A".into(),  // too short (min 2)
        },
        phone: Some("12345".into()), // too short for our custom validator
        skipped: "☃".into(),
    };

    let result = input.validate();
    assert!(result.is_err());
    let errors = result.unwrap_err();

    assert_eq!(errors.len(), 12);

    assert!(!errors.errors_for_field("name").is_empty());
    assert!(!errors.errors_for_field("short_name").is_empty());
    assert!(!errors.errors_for_field("count").is_empty());
    assert!(!errors.errors_for_field("email").is_empty());
    assert!(!errors.errors_for_field("website").is_empty());
    assert!(!errors.errors_for_field("uuid").is_empty());
    assert!(!errors.errors_for_field("alnum").is_empty());
    assert!(!errors.errors_for_field("ascii").is_empty());
    assert!(!errors.errors_for_field("choice").is_empty());

    assert!(!errors.errors_for_field("address.street").is_empty());
    assert!(!errors.errors_for_field("address.city").is_empty());

    assert!(!errors.errors_for_field("phone").is_empty());

    assert!(errors.errors_for_field("skipped").is_empty());
}

#[test]
fn test_option_required_none_is_error() {
    #[derive(Debug, Validate)]
    struct OptTest {
        #[validate(required)]
        opt: Option<String>,
    }

    let t = OptTest { opt: None };
    let res = t.validate();
    assert!(res.is_err());
    let errs = res.unwrap_err();
    let by_field = errs.errors_for_field("opt");
    assert_eq!(by_field.len(), 1);
    assert_eq!(by_field[0].code, "required");
}

#[test]
fn test_length_min_only() {
    #[derive(Debug, Validate)]
    struct LenMin {
        #[validate(length(min = 2))]
        s: String,
    }

    assert!(LenMin { s: "ab".into() }.validate().is_ok());
    assert!(LenMin { s: "a".into() }.validate().is_err());
}

#[test]
fn test_length_max_only() {
    #[derive(Debug, Validate)]
    struct LenMax {
        #[validate(length(max = 3))]
        s: String,
    }

    assert!(LenMax { s: "ab".into() }.validate().is_ok());
    assert!(LenMax { s: "abcd".into() }.validate().is_err());
}

#[test]
fn test_option_length_some_and_none() {
    #[derive(Debug, Validate)]
    struct OptLen {
        #[validate(length(min = 2))]
        s: Option<String>,
    }

    assert!(OptLen { s: None }.validate().is_ok());

    assert!(
        OptLen {
            s: Some("a".into())
        }
        .validate()
        .is_err()
    );

    assert!(
        OptLen {
            s: Some("ab".into())
        }
        .validate()
        .is_ok()
    );
}

#[test]
fn test_range_min_and_max_only() {
    #[derive(Debug, Validate)]
    struct RMin {
        #[validate(range(min = 5))]
        v: i32,
    }

    #[derive(Debug, Validate)]
    struct RMax {
        #[validate(range(max = 10))]
        v: i32,
    }

    assert!(RMin { v: 5 }.validate().is_ok());
    assert!(RMin { v: 4 }.validate().is_err());

    assert!(RMax { v: 10 }.validate().is_ok());
    assert!(RMax { v: 11 }.validate().is_err());
}

#[test]
fn test_pattern_good_and_bad() {
    #[derive(Debug, Validate)]
    struct P {
        #[validate(pattern = "^[0-9]{3}$")]
        code: String,
    }

    assert!(P { code: "123".into() }.validate().is_ok());
    let bad = P { code: "12".into() };
    assert!(bad.validate().is_err());
    let errs = bad.validate().unwrap_err();
    let by_field = errs.errors_for_field("code");
    assert_eq!(by_field[0].code, "pattern_mismatch");
}

#[test]
fn test_pattern_invalid_regex_reports_invalid_format() {
    #[derive(Debug, Validate)]
    struct PInvalid {
        #[validate(pattern = "*(invalid(")]
        code: String,
    }

    let val = PInvalid {
        code: "whatever".into(),
    };
    let res = val.validate();
    assert!(res.is_err());
    let errs = res.unwrap_err();
    let by_field = errs.errors_for_field("code");
    assert_eq!(by_field[0].code, "invalid_format");
    assert!(by_field[0].message.contains("invalid regex pattern"));
}

#[test]
fn test_identifier_good_and_bad() {
    #[derive(Debug, Validate)]
    struct IdTest {
        #[validate(identifier)]
        id: String,
    }

    assert!(IdTest { id: "abc_1".into() }.validate().is_ok());

    let empty = IdTest { id: "".into() };
    let errs = empty.validate().unwrap_err();
    assert_eq!(errs.errors_for_field("id")[0].code, "required");

    let bad = IdTest { id: "1abc".into() };
    let errs = bad.validate().unwrap_err();
    assert_eq!(errs.errors_for_field("id")[0].code, "invalid_format");
}

#[test]
fn test_nested_optional() {
    #[derive(Debug, Validate)]
    struct Outer {
        #[validate(nested)]
        child: Option<Address>,
    }

    assert!(Outer { child: None }.validate().is_ok());

    let bad = Outer {
        child: Some(Address {
            street: "".into(),
            city: "A".into(),
        }),
    };
    let errs = bad.validate().unwrap_err();
    assert!(!errs.errors_for_field("child.street").is_empty());
    assert!(!errs.errors_for_field("child.city").is_empty());
}

#[test]
fn test_one_of_option_and_bad() {
    #[derive(Debug, Validate)]
    struct OneOpt {
        #[validate(one_of("x", "y"))]
        choice: Option<String>,
    }

    assert!(OneOpt { choice: None }.validate().is_ok());

    assert!(
        OneOpt {
            choice: Some("x".into())
        }
        .validate()
        .is_ok()
    );

    let bad = OneOpt {
        choice: Some("z".into()),
    };
    let errs = bad.validate().unwrap_err();
    assert_eq!(errs.errors_for_field("choice")[0].code, "not_in_list");
}

#[test]
fn test_custom_multi_errors_merged_under_field() {
    #[derive(Debug, Validate)]
    struct C {
        #[validate(custom = "validate_multi_str")]
        val: String,
    }

    let bad = C { val: "".into() };
    let errs = bad.validate().unwrap_err();

    let list = errs.errors_for_field("val");
    assert!(!list.is_empty());
    assert!(list.iter().any(|e| e.code == "empty_field"));
}

#[test]
fn test_phone_none_is_ok() {
    #[derive(Debug, Validate)]
    struct PhoneOpt {
        #[validate(custom = "validate_phone")]
        phone: Option<String>,
    }

    assert!(PhoneOpt { phone: None }.validate().is_ok());
}

// Two alternatives with different fields: a struct of optionals would make invalid combinations representable.
#[derive(Debug, Validate)]
pub enum Enquiry {
    Product {
        #[validate(required, length(min = 2, max = 200))]
        organisation: String,
        #[validate(required, email)]
        contact_email: String,
    },
    MarketOpening {
        #[validate(required, email)]
        contact_email: String,
    },
    Anonymous,
}

#[test]
fn a_variant_is_validated_by_its_own_rules() {
    let ok = Enquiry::Product {
        organisation: "Acme".into(),
        contact_email: "hello@example.com".into(),
    };
    assert!(ok.validate().is_ok());

    let bad = Enquiry::Product {
        organisation: "V".into(),
        contact_email: "not-an-address".into(),
    };
    let errors = bad.validate().expect_err("both fields are invalid");
    assert_eq!(errors.len(), 2);
    assert!(!errors.errors_for_field("organisation").is_empty());
    assert!(!errors.errors_for_field("contact_email").is_empty());
}

#[test]
fn a_field_another_variant_requires_is_not_asked_for() {
    let petition = Enquiry::MarketOpening {
        contact_email: "hello@example.com".into(),
    };
    assert!(
        petition.validate().is_ok(),
        "a petition carries no organisation, so none can be missing"
    );
}

#[test]
fn the_error_path_is_the_field_the_caller_sent() {
    let petition = Enquiry::MarketOpening {
        contact_email: "nope".into(),
    };
    let errors = petition.validate().expect_err("the address is invalid");
    assert!(!errors.errors_for_field("contact_email").is_empty());
    assert!(
        errors
            .errors_for_field("market_opening.contact_email")
            .is_empty(),
        "a variant-qualified path would name something no caller ever sent"
    );
}

#[test]
fn a_variant_carrying_nothing_has_nothing_to_refuse() {
    assert!(Enquiry::Anonymous.validate().is_ok());
}

#[test]
fn an_optional_number_is_ranged_only_when_present() {
    #[derive(Debug, Validate)]
    struct Order {
        #[validate(range(min = 1, max = 9))]
        quantity: Option<u32>,
    }

    assert!(Order { quantity: None }.validate().is_ok());
    assert!(Order { quantity: Some(5) }.validate().is_ok());

    let errors = Order { quantity: Some(0) }
        .validate()
        .expect_err("below the minimum");
    assert!(!errors.errors_for_field("quantity").is_empty());
}

/// `deny`, because a warning in a generated arm cannot be silenced by its author.
#[deny(unused_variables)]
#[test]
fn an_unvalidated_field_is_not_bound() {
    #[derive(Validate)]
    #[allow(dead_code)]
    #[expect(
        clippy::allow_attributes,
        clippy::allow_attributes_without_reason,
        reason = "readability baseline"
    )]
    enum Signup {
        WithMetadata {
            #[validate(required, email)]
            contact_email: String,
            referrer: Option<String>,
            consented: bool,
        },
    }

    let request = Signup::WithMetadata {
        contact_email: "reader@example.com".into(),
        referrer: None,
        consented: true,
    };
    assert!(request.validate().is_ok());
}

mod rules {
    use fundamental::core::validate::{ValidationError, ValidationErrors};

    pub fn not_empty(value: &str) -> Result<(), ValidationErrors> {
        if value.is_empty() {
            let mut errors = ValidationErrors::new();
            errors.add(ValidationError::custom("empty", "must not be empty"));
            return Err(errors);
        }
        Ok(())
    }
}

#[test]
fn a_custom_validator_named_by_its_path_runs() {
    #[derive(Debug, Validate)]
    struct Named {
        #[validate(custom = "rules::not_empty")]
        name: String,
    }

    let errors = Named {
        name: String::new(),
    }
    .validate()
    .unwrap_err();
    assert_eq!(errors.errors_for_field("name")[0].code, "empty");
    assert!(Named { name: "a".into() }.validate().is_ok());
}

fn payout_names_its_payee(value: &Paid) -> ValidationResult<()> {
    if value.kind == "payout" && value.payee.is_none() {
        let mut errors = ValidationErrors::new();
        errors.add_for_field("payee", "required", "a payout names who it paid");
        return Err(errors);
    }
    Ok(())
}

#[derive(Debug, Validate)]
#[validate(custom = "payout_names_its_payee")]
struct Paid {
    #[validate(required)]
    kind: String,
    payee: Option<String>,
}

#[test]
fn a_rule_over_the_whole_value_names_the_field_it_refuses() {
    let errors = Paid {
        kind: "payout".into(),
        payee: None,
    }
    .validate()
    .unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors.errors_for_field("payee")[0].code, "required");
    assert!(
        Paid {
            kind: "spent".into(),
            payee: None
        }
        .validate()
        .is_ok()
    );
}

#[test]
fn a_rule_over_the_whole_value_runs_beside_the_fields_own() {
    let errors = Paid {
        kind: String::new(),
        payee: None,
    }
    .validate()
    .unwrap_err();
    assert_eq!(errors.errors_for_field("kind")[0].code, "required");
    let errors = Paid {
        kind: "payout".into(),
        payee: None,
    }
    .validate()
    .unwrap_err();
    assert_eq!(errors.errors_for_field("kind").len(), 0);
    assert_eq!(errors.errors_for_field("payee").len(), 1);
}
