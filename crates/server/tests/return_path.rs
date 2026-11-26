//! Every shape a `?next=` must be refused in.

use fundamental_server::{ReturnPath, ReturnPathError};

/// Every refusal, with the shape that produces it.
const REFUSED: &[(&str, &str)] = &[
    ("https://evil.test/", "an absolute URL"),
    ("//evil.test/", "protocol-relative"),
    (
        "/\\evil.test",
        "a backslash a browser normalises to a slash",
    ),
    ("/%2f%2fevil.test", "percent-encoded protocol-relative"),
    ("/%5cevil.test", "a percent-encoded backslash"),
    ("javascript:alert(1)", "a scheme that is not a path"),
    ("clients", "a bare word"),
    (
        "/clients\r\nLocation: https://evil.test",
        "response splitting",
    ),
    ("/clients\tfiles", "a control character"),
    ("/clients files", "whitespace"),
];

#[test]
fn every_open_redirect_shape_is_refused() {
    for (raw, why) in REFUSED {
        assert!(
            ReturnPath::parse(raw).is_err(),
            "`{raw}` was accepted, and it is {why}"
        );
    }
}

#[test]
fn each_refusal_names_what_arrived() {
    // Distinct reasons tell a probe from a typo in a log line.
    assert!(matches!(
        ReturnPath::parse("clients"),
        Err(ReturnPathError::MustBeAbsolutePath)
    ));
    assert!(matches!(
        ReturnPath::parse("//evil.test"),
        Err(ReturnPathError::ProtocolRelative)
    ));
    assert!(matches!(
        ReturnPath::parse("/%2f%2fevil.test"),
        Err(ReturnPathError::ProtocolRelative)
    ));
    assert!(matches!(
        ReturnPath::parse("/a\rb"),
        Err(ReturnPathError::NotAPath)
    ));
    assert!(matches!(
        ReturnPath::parse(&format!("/{}", "a".repeat(600))),
        Err(ReturnPathError::TooLong { .. })
    ));
}

#[test]
fn an_ordinary_path_survives() {
    for raw in ["/", "/clients", "/clients/42?tab=files", "/legal/dpa"] {
        let parsed = ReturnPath::parse(raw).unwrap_or_else(|_| panic!("`{raw}` must be accepted"));
        assert_eq!(parsed.as_location(), raw);
    }
}

#[test]
fn a_named_parameter_is_removed_and_the_rest_kept_in_order() {
    let cases = [
        ("/why?lang=nl", "/why"),
        ("/why?a=1&lang=nl&b=2", "/why?a=1&b=2"),
        ("/login?next=%2Fclients&lang=nl", "/login?next=%2Fclients"),
        ("/why?lang=nl&a=1&lang=en", "/why?a=1"),
        ("/why?lang&a=1", "/why?a=1"),
        // The `/legal` vs `/legalese` boundary, in a query.
        ("/why?language=nl", "/why?language=nl"),
        ("/why", "/why"),
    ];
    for (raw, expected) in cases {
        let stripped = ReturnPath::parse(raw)
            .unwrap_or_else(|_| panic!("`{raw}` must be accepted"))
            .without_query_param("lang");
        assert_eq!(stripped.as_location(), expected, "from `{raw}`");
    }
}

#[test]
fn stripping_the_only_parameter_leaves_no_trailing_question_mark() {
    let stripped = ReturnPath::parse("/why?lang=nl")
        .expect("accepted")
        .without_query_param("lang");
    assert!(!stripped.as_location().contains('?'));
    assert!(ReturnPath::parse(stripped.as_location()).is_ok());
}

#[test]
fn what_the_gate_builds_is_what_the_login_page_accepts() {
    let built = ReturnPath::parse("/clients/42?tab=files").expect("accepted");
    let encoded = built.encoded();
    // The gate writes `?next=<encoded>` and the page reads it back decoded.
    assert_eq!(encoded, "%2Fclients%2F42%3Ftab%3Dfiles");
    assert_eq!(
        ReturnPath::parse(built.as_location())
            .expect("accepted")
            .as_location(),
        "/clients/42?tab=files"
    );
}
