use std::fmt;

use crate::JobError;

/// What a job is about — the occurrence its deadline belongs to: `hold/7f3c…`,
/// `bouwcheck/42:answer`.
///
/// Name the occurrence, never the long-lived entity, and never personal data: it is stored in
/// plain text.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobSubject(String);

impl JobSubject {
    /// The subject `subject`.
    ///
    /// # Errors
    ///
    /// [`JobError::InvalidSubject`] outside `[A-Za-z0-9._:/-]{1,256}`.
    pub fn new(subject: impl Into<String>) -> Result<Self, JobError> {
        let subject = subject.into();
        if is_subject(&subject) {
            Ok(Self(subject))
        } else {
            Err(JobError::InvalidSubject {
                length: subject.chars().count(),
            })
        }
    }

    /// The subject as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for JobSubject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JobSubject({} chars)", self.0.chars().count())
    }
}

impl fmt::Display for JobSubject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether `subject` is a subject's stored form, `[A-Za-z0-9._:/-]{1,256}`.
pub(crate) fn is_subject(subject: &str) -> bool {
    (1..=256).contains(&subject.len())
        && subject
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '-'))
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn a_subject_is_a_bounded_printable_identifier() {
        assert_eq!(
            JobSubject::new("hold/7f3c:answer").expect("valid").as_str(),
            "hold/7f3c:answer"
        );
        assert!(JobSubject::new("x".repeat(256)).is_ok());
        for refused in [
            String::new(),
            "a b".to_owned(),
            "x".repeat(257),
            "jan@example.com".to_owned(),
            "ü".to_owned(),
        ] {
            assert_eq!(
                JobSubject::new(refused).expect_err("refused").error_code(),
                "JOBS_001"
            );
        }
    }

    #[test]
    fn a_subject_reaches_no_log_through_debug_or_its_error() {
        let subject = JobSubject::new("mailbox/occurrence-42").expect("valid");
        assert_eq!(format!("{subject:?}"), "JobSubject(21 chars)");
        let refused = JobSubject::new("jan@example.com").expect_err("refused");
        assert!(!format!("{refused:?}").contains("jan"));
        let developer = refused.developer_message().unwrap_or_default();
        assert!(developer.contains("15 characters"), "{developer}");
        assert!(!developer.contains("jan"));
    }
}
