//! [`EffectRow`] — an effect's claim on its name, and how far back its ledger is complete.

use chrono::{DateTime, Utc};
use fundamental_macros::Model;

/// One effect: the handler that applies it and its coverage watermark.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(
    sql,
    table = "fundamental_inbox_effects",
    check(
        name = "fundamental_inbox_effects_effect_length",
        expr = "char_length(effect) BETWEEN 1 AND 100"
    )
)]
pub(crate) struct EffectRow {
    /// The effect's declared name.
    #[model(id)]
    pub(crate) effect: String,
    /// The handler that claimed it: `module::path::handler`.
    pub(crate) handler: String,
    /// The watermark: an event older than this is not applied. `None` until the first sweep.
    pub(crate) covered_from: Option<DateTime<Utc>>,
    /// When `covered_from` last moved, by the database's clock.
    pub(crate) covered_from_set_at: Option<DateTime<Utc>>,
    /// The sweep deletes only rows processed before this.
    pub(crate) delete_below: Option<DateTime<Utc>>,
    /// When the effect was first claimed.
    pub(crate) registered_at: DateTime<Utc>,
}
