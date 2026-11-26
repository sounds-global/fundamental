use fundamental_core::AnyError;
use fundamental_core::validate::ValidationErrors;
use tracing::{error, info, warn};

use super::page_error::PageError;

/// What a loader answers instead of its model.
///
/// A `Failed` with a client status answers `404`, so a refusal never says the record exists.
#[derive(Debug)]
pub enum PageRefusal {
    /// The object is not there for this reader — absent, or not theirs.
    Unavailable,
    /// The page needs a signed-in reader.
    SignIn,
    /// The page needs the reader to confirm it is them.
    StepUp,
    /// The address's query does not fit what the page reads.
    Invalid(ValidationErrors),
    /// Something the loader called failed.
    Failed(AnyError),
}

impl<E: Into<AnyError>> From<E> for PageRefusal {
    fn from(error: E) -> Self {
        Self::Failed(error.into())
    }
}

impl PageRefusal {
    /// What a page's blocking query answers when it fails.
    #[cfg(feature = "ssr")]
    pub(crate) fn of_blocking_query(failure: AnyError) -> Self {
        let step_up = PageError::StepUpRequired;
        if failure.get().error_code() == fundamental_core::Error::error_code(&step_up) {
            Self::StepUp
        } else if failure.get().http_status_code() == 401 {
            Self::SignIn
        } else {
            Self::Failed(failure)
        }
    }

    /// The error this refusal is answered with on `route`, logged once here.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) fn into_answer(self, route: &str) -> AnyError {
        match self {
            Self::Unavailable => AnyError::new(PageError::Unavailable),
            Self::SignIn => AnyError::new(PageError::SignInRequired),
            Self::StepUp => AnyError::new(PageError::StepUpRequired),
            Self::Invalid(errors) => {
                // Field names and codes only: a query value is whatever the reader typed.
                let fields: Vec<&str> = errors
                    .errors()
                    .iter()
                    .map(|error| error.field.as_deref().unwrap_or("-"))
                    .collect();
                let codes: Vec<&str> = errors.errors().iter().map(|e| e.code.as_str()).collect();
                info!(route, ?fields, ?codes, "page query refused");
                AnyError::new(PageError::InvalidQuery { errors })
            }
            Self::Failed(failure) => {
                let (code, status) = (failure.get().error_code(), failure.get().http_status_code());
                if status >= 500 {
                    error!(route, code, status, error = %failure, "page loader failed");
                    failure
                } else {
                    warn!(
                        route,
                        original_code = code,
                        status,
                        "page loader refused with a client status; answered as not found"
                    );
                    AnyError::new(PageError::Unavailable)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PageError, PageRefusal};
    use fundamental_core::validate::{ValidationError, ValidationErrors};
    use fundamental_core::{AnyError, Error as _};
    use fundamental_server::ServerError;

    fn answer(refusal: PageRefusal) -> (&'static str, u16) {
        let error = refusal.into_answer("/teams/{team}");
        (error.get().error_code(), error.get().http_status_code())
    }

    #[test]
    fn every_refusal_about_the_object_is_one_not_found() {
        let forbidden = PageRefusal::Failed(AnyError::new(ServerError::unauthenticated()));
        assert_eq!(answer(PageRefusal::Unavailable), ("FW_PAGE_010", 404));
        assert_eq!(answer(forbidden), ("FW_PAGE_010", 404));
    }

    #[test]
    fn sign_in_and_step_up_lead_to_the_gate() {
        assert_eq!(answer(PageRefusal::SignIn), ("FW_PAGE_011", 401));
        assert_eq!(answer(PageRefusal::StepUp), ("FW_PAGE_012", 401));
    }

    #[test]
    fn a_blocking_query_refuses_as_a_loader_does() {
        let unauthenticated = AnyError::new(ServerError::Unauthenticated("no session".to_owned()));
        let failed = AnyError::new(ServerError::internal("the store is down"));
        assert_eq!(
            answer(PageRefusal::of_blocking_query(unauthenticated)),
            ("FW_PAGE_011", 401)
        );
        assert_eq!(answer(PageRefusal::of_blocking_query(failed)).1, 500);
    }

    #[test]
    fn a_blocking_query_asking_to_step_up_leads_to_the_step_up_gate() {
        let step_up = AnyError::new(PageError::StepUpRequired);
        assert_eq!(
            answer(PageRefusal::of_blocking_query(step_up)),
            ("FW_PAGE_012", 401)
        );
    }

    #[test]
    fn an_invalid_query_is_a_400() {
        let errors = ValidationErrors::single(ValidationError::required("page"));
        assert_eq!(answer(PageRefusal::Invalid(errors)), ("FW_PAGE_014", 400));
    }

    #[test]
    fn a_server_failure_keeps_its_own_answer() {
        let busy: PageRefusal = super::super::page_error::PageError::AuthorizerUnavailable {
            retry_after_secs: 5,
            source: AnyError::new(ServerError::unauthenticated()),
        }
        .into();
        assert_eq!(answer(busy), ("FW_PAGE_013", 503));
    }
}
