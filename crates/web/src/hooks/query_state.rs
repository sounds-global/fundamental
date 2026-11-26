use fundamental_core::AnyError;

/// The observable lifecycle of a query created by [`use_query!`](crate::use_query) or
/// [`use_query_prefetch!`](crate::use_query_prefetch).
#[derive(Clone, Debug)]
pub enum QueryState<T> {
    /// The query has not produced a value yet.
    Pending,
    /// The most recent request succeeded.
    Ready(T),
    /// A refetch is running while the previous successful value remains available.
    Refreshing(T),
    /// The latest request failed and may retain the previous successful value.
    Failed {
        /// The latest request error.
        error: AnyError,
        /// The last successful value, when one exists.
        stale: Option<T>,
    },
}

impl<T> QueryState<T> {
    pub(crate) fn stale_value(&self) -> Option<T>
    where
        T: Clone,
    {
        match self {
            Self::Ready(value) | Self::Refreshing(value) => Some(value.clone()),
            Self::Failed { stale, .. } => stale.clone(),
            Self::Pending => None,
        }
    }

    #[doc(hidden)]
    pub fn from_result(result: Result<T, AnyError>) -> Self {
        match result {
            Ok(value) => Self::Ready(value),
            Err(error) => Self::Failed { error, stale: None },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::QueryState;
    use fundamental_core::AnyError;
    use std::borrow::Cow;

    #[derive(Debug)]
    struct TestError;

    impl std::fmt::Display for TestError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("test error")
        }
    }

    impl std::error::Error for TestError {}

    impl fundamental_core::Error for TestError {
        fn error_code(&self) -> &'static str {
            "TEST_001"
        }

        fn display_message(&self) -> Cow<'_, str> {
            Cow::Borrowed("test error")
        }
    }

    #[test]
    fn successful_result_becomes_ready() {
        assert!(matches!(
            QueryState::from_result(Ok(42)),
            QueryState::Ready(42)
        ));
    }

    #[test]
    fn failed_result_has_no_stale_value_before_first_success() {
        let error = AnyError::new(TestError);
        let state = QueryState::<u8>::from_result(Err(error));

        assert!(matches!(state, QueryState::Failed { stale: None, .. }));
    }
}
