//! The model traits: [`Model`] and its backend extensions.

/// Model definitions and metadata specific to the YCQL backend.
pub mod cql;
/// Model definitions and metadata specific to the offline (local SQLite) backend.
pub mod offline;
/// Model definitions and metadata specific to the PostgreSQL-wire backend.
pub mod sql;

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

mod admit;
mod base32_layout;
mod bits128;
mod canonical_text;
mod database_type;
mod handle;
mod handle_prefix;
mod handle_refusal;
mod has_handle;
mod identifier;
mod index_def;
mod index_key;
mod markers;
mod opaque128;
mod row_bound;
mod spelled;
mod timestamp_precision;
mod type_id_text;
mod unadmitted;

pub use admit::Admit;
pub use bits128::Bits128;
pub use canonical_text::CanonicalText;
pub use database_type::DatabaseType;
pub use handle::Handle;
pub use handle_prefix::HandlePrefix;
pub use handle_refusal::HandleRefusal;
pub use has_handle::HasHandle;
pub use identifier::{GeneratedId, Identifier};
pub use index_def::IndexDef;
pub use index_key::IndexKey;
pub use opaque128::{MalformedOpaque128, Opaque128};
pub use row_bound::RowBound;
pub use spelled::Spelled;
pub use timestamp_precision::TimestampPrecision;
pub use type_id_text::TypeIdText;
pub use unadmitted::Unadmitted;

pub use markers::*;

/// A domain model, usable on every surface; usually derived with `#[derive(Model)]`.
pub trait Model: Sized {
    /// The name of this model type.
    const TYPE_NAME: &'static str;

    /// Columns backed by a [`Secret`](crate::secrets::Secret) family field, each with the `key_id` it is encrypted under.
    fn secret_columns() -> &'static [(&'static str, &'static str)] {
        &[]
    }

    /// The columns every encrypted field is sealed to, in column order.
    ///
    /// The SQL wire refuses a partial update of one of these unless it also erases every encrypted column.
    fn row_bound_columns() -> &'static [&'static str] {
        &[]
    }

    /// The columns an outbox row's seal covers, for a `#[model(outbox)]` model.
    ///
    /// The SQL wire refuses a partial update naming one of them, since it cannot re-seal the row.
    fn outbox_sealed_columns() -> Option<&'static [&'static str]> {
        None
    }

    /// Each `Localized` field's language that is missing or to check in `languages`.
    fn translation_gaps(
        &self,
        languages: &crate::i18n::Languages,
    ) -> Vec<crate::i18n::TranslationGap> {
        let _ = languages;
        Vec::new()
    }

    /// Refuses publishing while any [`translation_gaps`](Self::translation_gaps) remain.
    ///
    /// # Errors
    ///
    /// [`NotReadyToPublish`](crate::i18n::NotReadyToPublish), naming every gap.
    fn ready_to_publish(
        &self,
        languages: &crate::i18n::Languages,
    ) -> Result<(), crate::i18n::NotReadyToPublish> {
        let gaps = self.translation_gaps(languages);
        if gaps.is_empty() {
            Ok(())
        } else {
            Err(crate::i18n::NotReadyToPublish::new(Self::TYPE_NAME, gaps))
        }
    }
}
