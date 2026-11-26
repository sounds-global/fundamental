//! Integration tests for the ErrorOverride derive macro.
#![allow(missing_docs)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use fundamental::{Error, ErrorOverride};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ErrorOverride)]
pub enum ConstraintKind {
    #[overrides(status = 409)]
    Unique,
    ForeignKey,
    #[overrides(
        code = "CHECK_FAILED",
        message = "A check constraint failed",
        developer_message = "check constraint rejected the value",
        status = 422,
        retryable = false,
        exit_code = 65
    )]
    Check,
    #[overrides(retryable)]
    Deadlock,
}

#[test]
fn test_declared_keys_return_some() {
    assert_eq!(
        ConstraintKind::Unique.override_http_status_code(),
        Some(409)
    );

    assert_eq!(
        ConstraintKind::Check.override_error_code(),
        Some("CHECK_FAILED")
    );
    assert_eq!(
        ConstraintKind::Check.override_display_message().as_deref(),
        Some("A check constraint failed")
    );
    assert_eq!(
        ConstraintKind::Check
            .override_developer_message()
            .as_deref(),
        Some("check constraint rejected the value")
    );
    assert_eq!(ConstraintKind::Check.override_http_status_code(), Some(422));
    assert_eq!(ConstraintKind::Check.override_is_retryable(), Some(false));
    assert_eq!(ConstraintKind::Check.override_exit_code(), Some(65));

    assert_eq!(ConstraintKind::Deadlock.override_is_retryable(), Some(true));
}

#[test]
fn test_undeclared_keys_return_none() {
    assert_eq!(ConstraintKind::Unique.override_error_code(), None);
    assert_eq!(
        ConstraintKind::Unique.override_display_message().as_deref(),
        None
    );
    assert_eq!(ConstraintKind::Unique.override_is_retryable(), None);
    assert_eq!(ConstraintKind::Unique.override_exit_code(), None);

    assert_eq!(ConstraintKind::ForeignKey.override_error_code(), None);
    assert_eq!(ConstraintKind::ForeignKey.override_http_status_code(), None);
    assert_eq!(ConstraintKind::ForeignKey.override_is_retryable(), None);
}

#[derive(Debug, Error)]
pub enum DbError {
    #[error(
        code = "DB_003",
        message = "Query failed",
        developer_message = "constraint violation: {constraint}",
        status = 500
    )]
    ConstraintViolation {
        constraint: String,
        #[overrides]
        kind: ConstraintKind,
    },
}

#[test]
fn test_sugar_end_to_end() {
    let make = |kind| DbError::ConstraintViolation {
        constraint: "users_email_key".into(),
        kind,
    };

    let err = make(ConstraintKind::Unique);
    assert_eq!(err.http_status_code(), 409);
    assert_eq!(err.error_code(), "DB_003");
    assert_eq!(err.display_message(), "Query failed");
    assert_eq!(
        err.developer_message().as_deref(),
        Some("constraint violation: users_email_key")
    );

    let err = make(ConstraintKind::ForeignKey);
    assert_eq!(err.http_status_code(), 500);

    let err = make(ConstraintKind::Check);
    assert_eq!(err.http_status_code(), 422);
    assert_eq!(err.error_code(), "CHECK_FAILED");
    assert_eq!(err.display_message(), "A check constraint failed");
    assert_eq!(
        err.developer_message().as_deref(),
        Some("check constraint rejected the value")
    );
    assert!(!err.is_retryable());
    assert_eq!(err.exit_code(), 65);
}

#[derive(Debug, ErrorOverride)]
pub enum ShapedKind {
    #[overrides(status = 410)]
    Named { _reason: String },
    #[overrides(status = 411)]
    Tuple(u8),
}

#[test]
fn test_variants_with_fields() {
    let named = ShapedKind::Named {
        _reason: "gone".into(),
    };
    assert_eq!(named.override_http_status_code(), Some(410));
    assert_eq!(ShapedKind::Tuple(7).override_http_status_code(), Some(411));
}
