use fundamental_core::AnyError;
use leptos::prelude::*;

/// The count of writes that went through under one [`AsyncDefaults`](super::AsyncDefaults).
///
/// Every query tracks it, so the page's queries read again after a save; no keys.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct Writes(RwSignal<u64>);

impl Writes {
    /// The counter in context, if one was provided.
    #[must_use]
    pub fn current() -> Option<Self> {
        use_context()
    }

    /// Provides a counter in the current owner, or returns the one already in context.
    pub(crate) fn provide() -> Self {
        Self::current().unwrap_or_else(|| {
            let writes = Self(RwSignal::new(0));
            provide_context(writes);
            writes
        })
    }

    /// The writes made so far, tracked; `0` when no counter was provided.
    #[must_use]
    pub fn seen(writes: Option<Self>) -> u64 {
        writes.map_or(0, |writes| writes.0.get())
    }

    /// Tracks the writes made so far, so the reactive scope reading it runs again after one.
    pub fn track(writes: Option<Self>) {
        if let Some(writes) = writes {
            writes.0.track();
        }
    }

    /// Records the write when `answer` says it went through.
    pub(crate) fn record_if_written<T>(&self, answer: Option<&Result<T, AnyError>>) {
        if let Some(Ok(_)) = answer {
            self.0.update(|count| *count += 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::validate::ValidationErrors;

    #[test]
    fn a_write_that_went_through_is_counted() {
        let owner = Owner::new();
        owner.with(|| {
            let writes = Writes::provide();
            writes.record_if_written(Some(&Ok::<_, AnyError>(())));
            assert_eq!(Writes::seen(Some(writes)), 1);
        });
    }

    #[test]
    fn a_refused_write_or_no_answer_is_not_counted() {
        let owner = Owner::new();
        owner.with(|| {
            let writes = Writes::provide();
            writes.record_if_written(Some(&Err::<(), _>(AnyError::new(ValidationErrors::new()))));
            writes.record_if_written::<()>(None);
            assert_eq!(Writes::seen(Some(writes)), 0);
        });
    }

    #[test]
    fn providing_again_reuses_the_counter_in_context() {
        let owner = Owner::new();
        owner.with(|| {
            let outer = Writes::provide();
            let inner = Owner::new();
            inner.with(|| {
                Writes::provide().record_if_written(Some(&Ok::<_, AnyError>(())));
            });
            assert_eq!(Writes::seen(Some(outer)), 1);
        });
    }

    #[test]
    fn nothing_is_seen_without_a_counter() {
        assert_eq!(Writes::seen(None), 0);
    }

    #[cfg(feature = "ssr")]
    #[tokio::test]
    async fn a_prefetched_query_reads_again_after_a_write() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let _ = any_spawner::Executor::init_tokio();
        let owner = Owner::new();
        owner.set();
        crate::AsyncDefaultsConfig::default().provide();

        let reads = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&reads);
        let query = crate::use_query_prefetch!(move || {
            let counted = Arc::clone(&counted);
            async move { Ok::<_, AnyError>(counted.fetch_add(1, Ordering::SeqCst)) }
        });
        let resource = *query;
        assert_eq!(resource.await.ok(), Some(0));

        Writes::current()
            .expect("provided with the defaults")
            .record_if_written(Some(&Ok::<_, AnyError>(())));
        any_spawner::Executor::tick().await;

        assert_eq!(resource.await.ok(), Some(1));
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
}
