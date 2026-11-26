use fundamental_core::AnyError;
use fundamental_core::action::Outcome;
use leptos::prelude::*;

/// State and dispatch handle exposed to a [`Mutation`](super::Mutation) child.
#[derive(Clone)]
pub struct MutationScope<I: 'static, T: Send + Sync + 'static> {
    pub(super) submit_callback: Callback<I, ()>,
    pub(super) pending_signal: Memo<bool>,
    pub(super) value_signal: Signal<Option<Result<Outcome<T>, AnyError>>>,
}

impl<I: 'static, T: Send + Sync + 'static> MutationScope<I, T> {
    /// Dispatches the mutation with its input (`()` for a no-input action).
    pub fn submit(&self, input: I) {
        self.submit_callback.run(input);
    }

    /// Returns whether a mutation request is in flight.
    pub fn pending(&self) -> bool {
        self.pending_signal.get()
    }

    /// Returns the latest completed mutation result: the write's [`Outcome`] or its refusal.
    pub fn value(&self) -> Option<Result<Outcome<T>, AnyError>>
    where
        T: Clone,
    {
        self.value_signal.get()
    }

    /// Returns the latest write's outcome; `None` before the first answer and after a refusal.
    pub fn outcome(&self) -> Option<Outcome<()>>
    where
        T: Clone,
    {
        self.value_signal.with(|value| match value {
            Some(Ok(outcome)) => Some(outcome.clone().without_value()),
            _ => None,
        })
    }

    /// Returns the words that refuse `field` in the latest answer, to draw beside it.
    pub fn refusal(&self, field: &str) -> Option<String> {
        self.value_signal.with(|value| match value {
            Some(Err(error)) => error
                .get()
                .field_errors()
                .into_iter()
                .find(|refused| refused.field.as_deref() == Some(field))
                .map(|refused| refused.message),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::action::ChangeId;
    use fundamental_core::validate::ValidationErrors;

    fn scope(answer: Option<Result<Outcome<u8>, AnyError>>) -> MutationScope<(), u8> {
        MutationScope {
            submit_callback: Callback::new(|()| {}),
            pending_signal: Memo::new(|_| false),
            value_signal: Signal::derive(move || answer.clone()),
        }
    }

    fn refused() -> AnyError {
        let mut errors = ValidationErrors::new();
        errors.add_for_field("email", "taken", "Someone already has this address.");
        AnyError::new(errors)
    }

    #[test]
    fn refusal_gives_the_words_for_the_refused_field() {
        let owner = Owner::new();
        owner.with(|| {
            let scope = scope(Some(Err(refused())));
            assert_eq!(
                scope.refusal("email").as_deref(),
                Some("Someone already has this address.")
            );
        });
    }

    #[test]
    fn refusal_is_none_for_a_field_the_error_does_not_name() {
        let owner = Owner::new();
        owner.with(|| assert_eq!(scope(Some(Err(refused()))).refusal("name"), None));
    }

    #[test]
    fn refusal_is_none_after_a_write_that_went_through() {
        let owner = Owner::new();
        owner.with(|| assert_eq!(scope(Some(Ok(Outcome::changed(1)))).refusal("email"), None));
    }

    #[test]
    fn outcome_gives_the_change_and_the_words_of_the_latest_write() {
        let owner = Owner::new();
        owner.with(|| {
            let id: ChangeId = "67e55044-10b1-426f-9247-bb680e5fe0c8".parse().unwrap();
            let scope = scope(Some(Ok(Outcome::changed(1).change(id).said("Saved."))));
            assert_eq!(
                scope.outcome(),
                Some(Outcome::nothing().change(id).said("Saved."))
            );
        });
    }

    #[test]
    fn outcome_is_none_before_an_answer_and_after_a_refusal() {
        let owner = Owner::new();
        owner.with(|| {
            assert_eq!(scope(None).outcome(), None);
            assert_eq!(scope(Some(Err(refused()))).outcome(), None);
        });
    }
}
