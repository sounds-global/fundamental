//! Integration tests for the Error derive macro.
#![allow(missing_docs)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use fundamental::{Error, ErrorOverride};
use std::error::Error as StdError;
use std::fmt;

#[derive(Debug)]
pub struct SourceError(String);

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "source: {}", self.0)
    }
}

impl StdError for SourceError {}

#[derive(Debug, Error)]
pub enum TestError {
    #[error(code = "NOT_FOUND", message = "Resource not found", status = 404)]
    NotFound,

    #[error(
        code = "INVALID",
        message = "Invalid input",
        developer_message = "Constraint violation: {field} {reason}",
        status = 400
    )]
    InvalidInput { field: String, reason: String },

    #[error(code = "CONFLICT", message = "Conflict detected", status = 409)]
    Conflict(String),

    #[error(code = "INTERNAL", message = "Internal error", status = 500, retryable)]
    Internal(
        #[from]
        #[source]
        SourceError,
    ),

    #[error(code = "DB_ERROR", message = "Database error", status = 500)]
    DatabaseError(String, #[source] SourceError),
}

#[derive(Debug, Error)]
pub enum NamedFromError {
    #[error(code = "WRAPPED", message = "Wrapped error", status = 500)]
    Wrapped {
        #[from]
        #[source]
        inner: SourceError,
    },
}

#[test]
fn test_from_impl_named() {
    let source = SourceError("named error".into());
    let err: NamedFromError = source.into();

    assert_eq!(err.error_code(), "WRAPPED");
    let src = err.source().expect("should have source");
    assert!(src.to_string().contains("named error"));
}

#[test]
fn test_error_code() {
    assert_eq!(TestError::NotFound.error_code(), "NOT_FOUND");
    assert_eq!(
        TestError::InvalidInput {
            field: "email".into(),
            reason: "invalid".into()
        }
        .error_code(),
        "INVALID"
    );
    assert_eq!(TestError::Conflict("dup".into()).error_code(), "CONFLICT");
    assert_eq!(
        TestError::Internal(SourceError("oops".into())).error_code(),
        "INTERNAL"
    );
}

#[test]
fn test_developer_message() {
    assert_eq!(
        TestError::InvalidInput {
            field: "email".into(),
            reason: "invalid".into()
        }
        .developer_message()
        .as_deref(),
        Some("Constraint violation: email invalid")
    );
}

#[test]
fn test_display_message() {
    assert_eq!(TestError::NotFound.display_message(), "Resource not found");
    assert_eq!(
        TestError::InvalidInput {
            field: "email".into(),
            reason: "invalid".into()
        }
        .display_message(),
        "Invalid input"
    );
    assert_eq!(
        TestError::Conflict("dup".into()).display_message(),
        "Conflict detected"
    );
}

#[test]
fn test_http_status_code() {
    assert_eq!(TestError::NotFound.http_status_code(), 404);
    assert_eq!(
        TestError::InvalidInput {
            field: "email".into(),
            reason: "invalid".into()
        }
        .http_status_code(),
        400
    );
    assert_eq!(TestError::Conflict("dup".into()).http_status_code(), 409);
    assert_eq!(
        TestError::Internal(SourceError("oops".into())).http_status_code(),
        500
    );
}

#[test]
fn test_is_retryable() {
    assert!(!TestError::NotFound.is_retryable());
    assert!(!TestError::Conflict("dup".into()).is_retryable());
    assert!(TestError::Internal(SourceError("oops".into())).is_retryable());
}

#[test]
fn test_source() {
    assert!(TestError::NotFound.source().is_none());

    assert!(
        TestError::InvalidInput {
            field: "email".into(),
            reason: "invalid".into()
        }
        .source()
        .is_none()
    );

    let err = TestError::Internal(SourceError("oops".into()));
    let source = err.source().expect("should have source");
    assert!(source.to_string().contains("oops"));

    let err = TestError::DatabaseError("query failed".into(), SourceError("timeout".into()));
    let source = err.source().expect("should have source");
    assert!(source.to_string().contains("timeout"));
}

#[test]
fn test_display() {
    assert_eq!(format!("{}", TestError::NotFound), "Resource not found");
    assert_eq!(
        format!(
            "{}",
            TestError::InvalidInput {
                field: "email".into(),
                reason: "invalid".into()
            }
        ),
        "Invalid input"
    );
}

#[test]
fn test_from_impl_tuple() {
    let source = SourceError("test error".into());
    let err: TestError = source.into();

    assert_eq!(err.error_code(), "INTERNAL");
    assert!(err.is_retryable());
}

#[derive(Debug, Clone, Copy)]
pub enum Severity {
    Fatal,
    Minor,
}

impl ErrorOverride for Severity {
    fn override_http_status_code(&self) -> Option<u16> {
        match self {
            Self::Fatal => Some(503),
            Self::Minor => None, // fall back to the variant's declared status
        }
    }

    fn override_developer_message(&self) -> Option<std::borrow::Cow<'_, str>> {
        match self {
            Self::Fatal => Some(std::borrow::Cow::Borrowed("severity=fatal")),
            Self::Minor => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum OverrideError {
    #[error(
        code = "OV_001",
        message = "Something failed",
        developer_message = "default dev message: {detail}",
        status = 500
    )]
    Failure {
        detail: String,
        #[overrides]
        severity: Severity,
    },
}

#[test]
fn test_override_wins() {
    let err = OverrideError::Failure {
        detail: "boom".into(),
        severity: Severity::Fatal,
    };
    assert_eq!(err.http_status_code(), 503);
    assert_eq!(err.developer_message().as_deref(), Some("severity=fatal"));
    assert_eq!(err.error_code(), "OV_001");
    assert_eq!(err.display_message(), "Something failed");
}

#[test]
fn test_override_falls_back() {
    let err = OverrideError::Failure {
        detail: "boom".into(),
        severity: Severity::Minor,
    };
    assert_eq!(err.http_status_code(), 500);
    assert_eq!(
        err.developer_message().as_deref(),
        Some("default dev message: boom")
    );
}

#[derive(Debug, Clone, Copy)]
struct StatusHint(Option<u16>);

impl ErrorOverride for StatusHint {
    fn override_http_status_code(&self) -> Option<u16> {
        self.0
    }
}

#[derive(Debug, Error)]
enum MultiOverrideError {
    #[error(code = "MO_001", message = "multi", status = 500)]
    Two {
        #[overrides]
        primary: StatusHint,
        #[overrides]
        secondary: StatusHint,
    },
}

#[test]
fn test_multiple_overrides_precedence() {
    let err = MultiOverrideError::Two {
        primary: StatusHint(Some(409)),
        secondary: StatusHint(Some(418)),
    };
    assert_eq!(err.http_status_code(), 409);

    let err = MultiOverrideError::Two {
        primary: StatusHint(None),
        secondary: StatusHint(Some(418)),
    };
    assert_eq!(err.http_status_code(), 418);

    let err = MultiOverrideError::Two {
        primary: StatusHint(None),
        secondary: StatusHint(None),
    };
    assert_eq!(err.http_status_code(), 500);
}

#[derive(Debug, Error)]
pub enum InnerError {
    #[error(
        code = "INNER_001",
        message = "Inner failed",
        developer_message = "inner detail: {detail}",
        status = 404,
        retryable
    )]
    WithDev { detail: String },

    #[error(code = "INNER_002", message = "Inner conflict", status = 409)]
    NoDev,
}

#[derive(Debug, Error)]
pub enum OuterError {
    #[error(
        code = "OUTER_001",
        message = "Outer failed",
        developer_message = "outer dev message",
        status = 500
    )]
    Wrapped {
        #[overrides]
        inner: InnerError,
    },

    Transparent {
        #[overrides]
        inner: InnerError,
    },
}

#[test]
fn test_error_delegation() {
    let err = OuterError::Wrapped {
        inner: InnerError::WithDev {
            detail: "boom".into(),
        },
    };
    assert_eq!(err.error_code(), "INNER_001");
    assert_eq!(err.display_message(), "Inner failed");
    assert_eq!(err.http_status_code(), 404);
    assert!(err.is_retryable());
    assert_eq!(
        err.developer_message().as_deref(),
        Some("inner detail: boom")
    );

    let err = OuterError::Wrapped {
        inner: InnerError::NoDev,
    };
    assert_eq!(err.error_code(), "INNER_002");
    assert_eq!(err.http_status_code(), 409);
    assert!(!err.is_retryable());
    assert_eq!(
        err.developer_message().as_deref(),
        Some("outer dev message")
    );
}

#[test]
fn test_transparent_variant_without_error_attribute() {
    let err = OuterError::Transparent {
        inner: InnerError::NoDev,
    };
    assert_eq!(err.error_code(), "INNER_002");
    assert_eq!(err.display_message(), "Inner conflict");
    assert_eq!(err.http_status_code(), 409);
}

#[derive(Debug, Error)]
pub enum OptionalOverrideError {
    #[error(code = "OPT_001", message = "optional", status = 500)]
    Maybe {
        #[overrides]
        severity: Option<Severity>,
    },
}

#[test]
fn test_option_override() {
    let make = |severity| OptionalOverrideError::Maybe { severity };

    assert_eq!(make(Some(Severity::Fatal)).http_status_code(), 503);
    assert_eq!(make(Some(Severity::Minor)).http_status_code(), 500);
    assert_eq!(make(None).http_status_code(), 500);
}

#[test]
fn test_from_with_question_mark() {
    fn may_fail() -> Result<(), SourceError> {
        Err(SourceError("failure".into()))
    }

    fn wrapper() -> Result<(), TestError> {
        may_fail()?;
        Ok(())
    }

    let result = wrapper();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.error_code(), "INTERNAL");
}

#[derive(Debug, Error)]
pub enum OptionalInterpError {
    #[error(
        code = "OPT_I01",
        message = "table {table} col {col}",
        developer_message = "dev {table}",
        status = 400
    )]
    Missing { table: Option<String>, col: String },

    #[error(code = "OPT_I02", message = "id {0}", status = 400)]
    ById(Option<i32>),
}

#[test]
fn test_option_interpolation_some() {
    let err = OptionalInterpError::Missing {
        table: Some("users".into()),
        col: "email".into(),
    };

    assert_eq!(err.display_message(), "table users col email");
    assert_eq!(err.to_string(), "table users col email");
    assert_eq!(err.developer_message().as_deref(), Some("dev users"));
}

#[test]
fn test_option_interpolation_none() {
    let err = OptionalInterpError::Missing {
        table: None,
        col: "email".into(),
    };

    assert_eq!(err.display_message(), "table None(table) col email");
    assert_eq!(err.to_string(), "table None(table) col email");
    assert_eq!(err.developer_message().as_deref(), Some("dev None(table)"));
}

#[test]
fn test_option_interpolation_tuple() {
    assert_eq!(OptionalInterpError::ById(Some(7)).to_string(), "id 7");
    assert_eq!(OptionalInterpError::ById(None).to_string(), "id None(_0)");
}

#[test]
fn test_option_interpolation_validation_error() {
    use fundamental::core::validate::ValidationError;

    let some: ValidationError = OptionalInterpError::Missing {
        table: Some("users".into()),
        col: "email".into(),
    }
    .into();
    assert_eq!(some.message, "table users col email");
    assert_eq!(some.developer_message.as_deref(), Some("dev users"));

    let none: ValidationError = OptionalInterpError::Missing {
        table: None,
        col: "email".into(),
    }
    .into();
    assert_eq!(none.message, "table None(table) col email");
    assert_eq!(none.developer_message.as_deref(), Some("dev None(table)"));
}

#[derive(Debug, Error)]
pub enum ChainError {
    Database {
        #[from]
        #[overrides]
        error: InnerError,
    },
}

#[test]
fn test_display_transparency() {
    // The override short-circuits the declared template in Display too.
    let err = OuterError::Transparent {
        inner: InnerError::NoDev,
    };
    assert_eq!(err.to_string(), "Inner conflict");

    let err = OuterError::Wrapped {
        inner: InnerError::WithDev {
            detail: "boom".into(),
        },
    };
    assert_eq!(err.to_string(), "Inner failed");
    assert_eq!(err.to_string(), err.display_message());
}

#[test]
fn test_from_implies_source() {
    let err: ChainError = InnerError::NoDev.into();
    let source = err.source().expect("#[from] should imply #[source]");
    assert!(source.downcast_ref::<InnerError>().is_some());
    assert_eq!(source.to_string(), "Inner conflict");
}

#[test]
fn test_transparent_wrapper_end_to_end() {
    use fundamental::core::validate::ValidationError;

    let err: ChainError = InnerError::NoDev.into();

    assert_eq!(err.error_code(), "INNER_002");
    assert_eq!(err.to_string(), "Inner conflict");
    assert_eq!(err.http_status_code(), 409);
    assert!(err.source().is_some());

    let ve: ValidationError = err.into();
    assert_eq!(ve.code, "INNER_002");
    assert_eq!(ve.message, "Inner conflict");
    assert_eq!(ve.developer_message, None);
}

#[test]
fn test_box_dyn_std_error() {
    let boxed: Box<dyn StdError + Send + Sync> = Box::new(TestError::NotFound);
    assert_eq!(boxed.to_string(), "Resource not found");

    let boxed: Box<dyn StdError> = Box::new(InnerError::NoDev);
    assert!(boxed.source().is_none());
}

use fundamental::core::error::AnyError;

#[derive(Debug, Error)]
pub enum TargetError {
    #[error(code = "TGT_001", message = "Target failed", status = 500)]
    Own,

    Other {
        #[from]
        #[overrides]
        error: AnyError,
    },
}

#[derive(Debug, Error)]
#[error_into(TargetError)]
pub enum DriverError {
    #[error(
        code = "DRV_001",
        message = "Driver conflict: {detail}",
        developer_message = "driver dev: {detail}",
        status = 409,
        retryable
    )]
    Conflict { detail: String },
}

#[test]
fn test_any_error_delegation() {
    let any = AnyError::new(DriverError::Conflict {
        detail: "dup".into(),
    });

    assert_eq!(any.to_string(), "Driver conflict: dup");
    assert_eq!(any.override_error_code(), Some("DRV_001"));
    assert_eq!(
        any.override_display_message().as_deref(),
        Some("Driver conflict: dup")
    );
    assert_eq!(
        any.override_developer_message().as_deref(),
        Some("driver dev: dup")
    );
    assert_eq!(any.override_http_status_code(), Some(409));
    assert_eq!(any.override_is_retryable(), Some(true));
    let inner = any
        .downcast_ref::<DriverError>()
        .expect("downcast should recover the wrapped type");
    assert!(matches!(inner, DriverError::Conflict { .. }));
}

#[test]
fn test_error_into_question_mark() {
    fn driver_op() -> Result<(), DriverError> {
        Err(DriverError::Conflict {
            detail: "dup".into(),
        })
    }

    fn target_op() -> Result<(), TargetError> {
        driver_op()?;
        Ok(())
    }

    let err = target_op().unwrap_err();

    assert_eq!(err.error_code(), "DRV_001");
    assert_eq!(err.display_message(), "Driver conflict: dup");
    assert_eq!(err.to_string(), "Driver conflict: dup");
    assert_eq!(err.developer_message().as_deref(), Some("driver dev: dup"));
    assert_eq!(err.http_status_code(), 409);
    assert!(err.is_retryable());

    let source = err.source().expect("catch-all should chain");
    let any = source
        .downcast_ref::<AnyError>()
        .expect("source should be the AnyError carrier");
    assert!(any.downcast_ref::<DriverError>().is_some());
}

#[test]
fn test_error_into_validation_error() {
    use fundamental::core::validate::ValidationError;

    let err = TargetError::from(DriverError::Conflict {
        detail: "dup".into(),
    });
    let ve: ValidationError = err.into();
    assert_eq!(ve.code, "DRV_001");
    assert_eq!(ve.message, "Driver conflict: dup");
    assert_eq!(ve.developer_message.as_deref(), Some("driver dev: dup"));
}

#[derive(Debug, Error)]
pub enum ParamFilterError {
    #[error(code = "PF_001", message = "login failed for {username}")]
    Login { username: String, password: String },

    #[error(code = "PF_002", message = "token rejected: {token}")]
    Token {
        #[error(skip_param)]
        token: String,
    },
}

#[test]
fn validation_params_exclude_unreferenced_and_skipped_fields() {
    use fundamental::core::validate::ValidationError;

    let ve: ValidationError = ParamFilterError::Login {
        username: "alice".into(),
        password: "hunter2".into(),
    }
    .into();
    assert!(
        ve.params.contains_key("username"),
        "a template-referenced field must be a param"
    );
    assert!(
        !ve.params.contains_key("password"),
        "an unreferenced field must NOT be baked into params"
    );
    assert!(
        !format!("{:?}", ve.params).contains("hunter2"),
        "the unreferenced secret value must not appear anywhere in params"
    );

    let ve2: ValidationError = ParamFilterError::Token {
        token: "secret-token".into(),
    }
    .into();
    assert!(
        !ve2.params.contains_key("token"),
        "a #[error(skip_param)] field must NOT be baked into params"
    );
    assert!(
        !format!("{:?}", ve2.params).contains("secret-token"),
        "the skipped secret value must not appear in params"
    );
    assert_eq!(ve2.message, "token rejected: secret-token");
}

#[test]
fn test_error_into_composes_through_layers() {
    let layered = OuterError::Transparent {
        inner: InnerError::NoDev,
    };
    let err = TargetError::from(AnyError::new(layered));

    assert_eq!(err.error_code(), "INNER_002");
    assert_eq!(err.to_string(), "Inner conflict");
    assert_eq!(err.http_status_code(), 409);
}

#[derive(Debug, Error)]
pub enum BareCatchAllError {
    #[error(code = "BARE_001", message = "own failure", status = 500)]
    Own,

    Other(AnyError),
}

#[test]
fn test_implicit_catch_all() {
    let err = BareCatchAllError::from(AnyError::new(InnerError::NoDev));

    assert_eq!(err.error_code(), "INNER_002");
    assert_eq!(err.display_message(), "Inner conflict");
    assert_eq!(err.to_string(), "Inner conflict");
    assert_eq!(err.http_status_code(), 409);

    let source = err.source().expect("implicit catch-all should chain");
    assert!(source.downcast_ref::<AnyError>().is_some());
}

#[derive(Debug, Error)]
pub enum DeclaredAnyError {
    #[error(code = "DECL_001", message = "declared message", status = 502)]
    Wrapped { error: AnyError },
}

#[test]
fn test_catch_all_not_implied_with_explicit_attr() {
    let err = DeclaredAnyError::Wrapped {
        error: AnyError::new(InnerError::NoDev),
    };
    assert_eq!(err.error_code(), "DECL_001");
    assert_eq!(err.display_message(), "declared message");
    assert_eq!(err.http_status_code(), 502);
}

#[derive(Debug, Error)]
#[error_into(TargetError)]
pub enum CircuitBreakerError<E> {
    #[error(
        code = "CIRCUIT_001",
        message = "Circuit breaker is open",
        status = 503
    )]
    Open,

    #[error(code = "CIRCUIT_002", message = "Operation failed: {0}", status = 500)]
    Inner(#[source] E),

    Other(AnyError),
}

#[test]
fn test_generic_enum() {
    let open: CircuitBreakerError<InnerError> = CircuitBreakerError::Open;
    assert_eq!(open.error_code(), "CIRCUIT_001");
    assert_eq!(open.http_status_code(), 503);

    let inner = CircuitBreakerError::Inner(InnerError::NoDev);
    assert_eq!(inner.error_code(), "CIRCUIT_002");
    assert_eq!(inner.to_string(), "Operation failed: Inner conflict");
    assert_eq!(inner.http_status_code(), 500);
    let source = inner.source().expect("#[source] should chain");
    assert!(source.downcast_ref::<InnerError>().is_some());

    let other: CircuitBreakerError<InnerError> =
        CircuitBreakerError::from(AnyError::new(InnerError::NoDev));
    assert_eq!(other.error_code(), "INNER_002");
}

#[test]
fn test_generic_enum_error_into() {
    fn circuit_op() -> Result<(), CircuitBreakerError<InnerError>> {
        Err(CircuitBreakerError::Inner(InnerError::NoDev))
    }

    fn target_op() -> Result<(), TargetError> {
        circuit_op()?;
        Ok(())
    }

    let err = target_op().unwrap_err();
    assert_eq!(err.error_code(), "CIRCUIT_002");
    assert_eq!(err.to_string(), "Operation failed: Inner conflict");
    assert_eq!(err.http_status_code(), 500);
}

#[derive(Debug, Error)]
pub enum ThrottleError {
    #[error(
        code = "THR_001",
        message = "Too many requests",
        status = 429,
        retryable,
        retry_after = retry_after_secs
    )]
    RateLimited {
        retry_after_secs: u64,
    },

    #[error(
        code = "THR_002",
        message = "Upstream unavailable",
        status = 503,
        retryable
    )]
    Unavailable,

    #[error(code = "THR_003", message = "Malformed request", status = 400)]
    Malformed,

    Other(AnyError),
}

#[test]
fn test_retry_after_reports_the_declared_field() {
    let limited = ThrottleError::RateLimited {
        retry_after_secs: 42,
    };
    assert!(limited.is_retryable());
    assert_eq!(limited.retry_after_secs(), Some(42));
    assert_eq!(limited.http_status_code(), 429);
}

#[test]
fn test_retry_after_is_none_without_the_attribute() {
    // Silence must never read as "retry immediately".
    assert!(ThrottleError::Unavailable.is_retryable());
    assert_eq!(ThrottleError::Unavailable.retry_after_secs(), None);
    assert_eq!(ThrottleError::Malformed.retry_after_secs(), None);
}

#[test]
fn test_retry_after_survives_the_any_error_carrier() {
    // A delay lost at a `?` boundary would turn "retry in 42 s" into "retry whenever".
    let carried = TargetError::from(AnyError::new(ThrottleError::RateLimited {
        retry_after_secs: 42,
    }));
    assert_eq!(carried.error_code(), "THR_001");
    assert!(carried.is_retryable());
    assert_eq!(carried.retry_after_secs(), Some(42));
}

#[derive(Debug, Error)]
pub enum CodeError {
    #[error(
        code = "COD_001",
        message = "That code is not right. You have {attempts_left} attempts left.",
        status = 400
    )]
    Wrong { attempts_left: u32 },

    #[error(
        code = "COD_002",
        message = "We sent a code to {address}.",
        status = 400
    )]
    Sent { address: String },

    #[error(code = "COD_003", message = "Wait {seconds:?} seconds.", status = 429)]
    Formatted { seconds: u64 },

    #[error(transparent)]
    Inner(#[from] AnyError),
}

#[test]
fn test_message_numbers_names_each_whole_number_of_the_message() {
    let wrong = CodeError::Wrong { attempts_left: 2 };
    assert_eq!(wrong.message_numbers(), vec![("attempts_left", 2)]);
}

#[test]
fn test_message_numbers_never_carries_a_string_or_a_formatted_value() {
    let sent = CodeError::Sent {
        address: "jan@example.nl".to_owned(),
    };
    assert!(sent.message_numbers().is_empty());
    assert!(
        CodeError::Formatted { seconds: 3 }
            .message_numbers()
            .is_empty()
    );
}

#[test]
fn test_message_numbers_come_with_a_message_taken_from_an_inner_error() {
    let inner = CodeError::Inner(AnyError::new(CodeError::Wrong { attempts_left: 1 }));
    assert_eq!(inner.error_code(), "COD_001");
    assert_eq!(inner.message_numbers(), vec![("attempts_left", 1)]);
}

#[test]
fn test_field_errors_come_from_an_inner_error_through_transparent() {
    let mut refused = fundamental::core::validate::ValidationErrors::new();
    refused.add_for_field("email", "taken", "Someone already has this address.");
    let inner = CodeError::Inner(AnyError::new(refused.clone()));
    assert_eq!(inner.field_errors(), refused.errors().to_vec());
}

#[test]
fn test_field_errors_are_none_for_a_variant_that_names_no_field() {
    assert!(
        CodeError::Wrong { attempts_left: 2 }
            .field_errors()
            .is_empty()
    );
}

#[derive(Debug, Error)]
pub enum TeamError {
    #[error(
        code = "TEAM_003",
        field = "email",
        message = "Someone on the team already has this address.",
        status = 409
    )]
    EmailTaken,

    #[error(
        code = "TEAM_004",
        field = "name",
        message = "{name} is too long.",
        status = 400
    )]
    NameTooLong { name: String },
}

#[test]
fn test_field_errors_name_the_declared_field_with_the_user_safe_message() {
    let refused = TeamError::EmailTaken.field_errors();
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].field.as_deref(), Some("email"));
    assert_eq!(refused[0].code, "TEAM_003");
    assert_eq!(
        refused[0].message,
        "Someone on the team already has this address."
    );
}

#[test]
fn test_field_errors_carry_the_interpolated_message() {
    let refused = TeamError::NameTooLong {
        name: "Jan".to_owned(),
    }
    .field_errors();
    assert_eq!(refused[0].field.as_deref(), Some("name"));
    assert_eq!(refused[0].message, "Jan is too long.");
}

#[test]
fn test_a_field_variant_keeps_its_field_as_a_validation_error() {
    let converted: fundamental::core::validate::ValidationError = TeamError::EmailTaken.into();
    assert_eq!(converted.field.as_deref(), Some("email"));
}

#[derive(Debug, Error)]
pub enum SignUpError {
    #[error(
        code = "SIGNUP_001",
        field = "email",
        message = "Someone already has this address.",
        status = 409
    )]
    EmailTaken {
        #[overrides]
        cause: AnyError,
    },
}

#[test]
fn test_a_declared_field_is_kept_when_its_override_refuses_none() {
    let taken = SignUpError::EmailTaken {
        cause: AnyError::new(TeamError::NameTooLong {
            name: "Jan".to_owned(),
        }),
    };
    let refused = SignUpError::EmailTaken {
        cause: AnyError::new(CodeError::Wrong { attempts_left: 1 }),
    }
    .field_errors();
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].field.as_deref(), Some("email"));
    assert_eq!(taken.field_errors()[0].field.as_deref(), Some("name"));
}
