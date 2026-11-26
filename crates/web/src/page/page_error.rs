use fundamental_core::AnyError;
use fundamental_core::validate::ValidationErrors;
use fundamental_macros::Error;

/// Why a page was not rendered, as the router answers it.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PageError {
    /// Every refusal a loader makes about its object, so the page cannot tell absent from someone else's.
    #[error(code = "FW_PAGE_010", message = "This page is not here.", status = 404)]
    Unavailable,

    /// The page needs a signed-in reader.
    #[error(
        code = "FW_PAGE_011",
        message = "Sign in to see this page.",
        status = 401
    )]
    SignInRequired,

    /// The page needs the signed-in reader to confirm it is them.
    #[error(
        code = "FW_PAGE_012",
        message = "Confirm it is you to see this page.",
        status = 401
    )]
    StepUpRequired,

    /// The authorizer could not answer, so nobody was let through.
    #[error(
        code = "FW_PAGE_013",
        message = "This page cannot be opened right now. Try again in a moment.",
        status = 503,
        retryable,
        retry_after = retry_after_secs
    )]
    AuthorizerUnavailable {
        /// How long to wait before asking again.
        retry_after_secs: u64,
        /// What the authorizer answered.
        #[source]
        source: AnyError,
    },

    /// The address's query does not fit what the page reads.
    #[error(
        code = "FW_PAGE_014",
        message = "This address asks for something the page does not understand.",
        status = 400
    )]
    InvalidQuery {
        /// Every field that did not fit.
        errors: ValidationErrors,
    },

    /// A loader asked for an object no guard granted; refused at boot, so the route bypassed the checks.
    #[error(
        code = "FW_PAGE_015",
        message = "This page cannot be shown.",
        developer_message = "no guard granted {namespace}#{relation} on this request",
        status = 500
    )]
    ObjectNotGranted {
        /// The namespace asked for.
        namespace: &'static str,
        /// The relation asked for.
        relation: &'static str,
    },

    /// A loader read a parameter its policies never declared public.
    #[error(
        code = "FW_PAGE_016",
        message = "This page cannot be shown.",
        developer_message = "the parameter `{param}` is not declared public by any policy of this route",
        status = 500
    )]
    UndeclaredParam {
        /// The parameter read.
        param: String,
    },

    /// A guarded route is served over a state with no authorizer; refused at compile time otherwise.
    #[error(
        code = "FW_PAGE_017",
        message = "This page cannot be shown.",
        developer_message = "the guarded route {route} has no authorizer to ask",
        status = 500
    )]
    GuardedRouteWithoutAuthorizer {
        /// The route template.
        route: String,
    },
}

#[cfg(test)]
mod tests {
    use super::PageError;
    use fundamental_core::validate::ValidationErrors;
    use fundamental_core::{AnyError, Error as _};

    #[test]
    fn each_answer_leads_to_its_page() {
        let source = AnyError::new(PageError::Unavailable);
        let cases = [
            (PageError::Unavailable, "FW_PAGE_010", 404),
            (PageError::SignInRequired, "FW_PAGE_011", 401),
            (PageError::StepUpRequired, "FW_PAGE_012", 401),
            (
                PageError::AuthorizerUnavailable {
                    retry_after_secs: 5,
                    source,
                },
                "FW_PAGE_013",
                503,
            ),
            (
                PageError::InvalidQuery {
                    errors: ValidationErrors::new(),
                },
                "FW_PAGE_014",
                400,
            ),
            (
                PageError::ObjectNotGranted {
                    namespace: "team",
                    relation: "admin",
                },
                "FW_PAGE_015",
                500,
            ),
            (
                PageError::UndeclaredParam {
                    param: "project".into(),
                },
                "FW_PAGE_016",
                500,
            ),
            (
                PageError::GuardedRouteWithoutAuthorizer {
                    route: "/teams/{team}".into(),
                },
                "FW_PAGE_017",
                500,
            ),
        ];
        for (error, code, status) in cases {
            assert_eq!(error.error_code(), code);
            assert_eq!(error.http_status_code(), status);
            assert_eq!(error.is_retryable(), code == "FW_PAGE_013", "{code}");
        }
    }

    #[test]
    fn an_unavailable_authorizer_says_when_to_ask_again() {
        let error = PageError::AuthorizerUnavailable {
            retry_after_secs: 5,
            source: AnyError::new(PageError::Unavailable),
        };
        assert_eq!(error.retry_after_secs(), Some(5));
    }
}
