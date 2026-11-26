//! [`Binding`] — which effect a listener applies, from which consumer, at which isolation.

use fundamental_db::IsolationLevel;

use crate::{EffectName, InboxError};

/// What `#[subscribe(stream, consumer, inbox = "…", isolation = …)]` declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    effect: EffectName,
    handler: &'static str,
    stream: &'static str,
    consumer: &'static str,
    isolation: IsolationLevel,
}

impl Binding {
    /// A binding for `effect`, applied by `handler` (its `module::path::name`), delivered by
    /// `consumer` on `stream`, run at `isolation`.
    ///
    /// # Errors
    ///
    /// [`InboxError::InvalidEffectName`] if `effect` is not an effect name.
    pub fn new(
        effect: &'static str,
        handler: &'static str,
        stream: &'static str,
        consumer: &'static str,
        isolation: IsolationLevel,
    ) -> Result<Self, InboxError> {
        Ok(Self {
            effect: EffectName::new(effect)?,
            handler,
            stream,
            consumer,
            isolation,
        })
    }

    /// The effect's ledger name.
    #[must_use]
    pub fn effect(&self) -> EffectName {
        self.effect
    }

    /// The handler that applies the effect, as `module::path::name` — what claims the effect's
    /// name on the database.
    #[must_use]
    pub fn handler(&self) -> &'static str {
        self.handler
    }

    /// The stream the consumer reads.
    #[must_use]
    pub fn stream(&self) -> &'static str {
        self.stream
    }

    /// The durable consumer.
    #[must_use]
    pub fn consumer(&self) -> &'static str {
        self.consumer
    }

    /// The isolation level the effect's transaction runs at.
    #[must_use]
    pub fn isolation(&self) -> IsolationLevel {
        self.isolation
    }
}
