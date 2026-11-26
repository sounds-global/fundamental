//! Localization engine for the Fundamental framework.
//!
//! # Quick start
//!
//! ```
//! use std::sync::Arc;
//! use fundamental_i18n::{Catalog, Locale, install, m, with};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // An app registers each crate's messages: `.with_messages(&crate::m::CATALOGS)`.
//! let catalog = Catalog::builder("en".parse()?).build();
//! let nl: Locale = "nl".parse()?;
//!
//! // Scope a Dutch translator for a single call / test…
//! let said = with(Arc::new(catalog.translator(&nl)), || {
//!     m::validate::required("E-mail").to_string()
//! });
//! assert_eq!(said, "E-mail is verplicht");
//!
//! // …or install it process-wide.
//! install(catalog.translator(&nl));
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::sync::Arc;

mod catalog;
mod count_formatter;
mod date_style;
mod date_time_formatter;
mod day_style;
mod money_formatter;
mod money_style;
mod relative_unit;
mod relative_words;
mod time_style;
mod translator;

pub use catalog::{Catalog, CatalogBuilder};
pub use count_formatter::CountFormatter;
pub use date_style::DateStyle;
pub use date_time_formatter::DateTimeFormatter;
pub use day_style::DayStyle;
pub use money_formatter::MoneyFormatter;
pub use money_style::MoneyStyle;
pub use relative_unit::RelativeUnit;
pub use time_style::TimeStyle;
pub use translator::CatalogTranslator;

/// An IANA time zone from the database compiled into the framework.
#[doc(no_inline)]
pub use chrono_tz::Tz;

/// The date and time types [`DateTimeFormatter`] reads.
pub use chrono;

/// Key/placeholder helpers shared with the error derive.
#[doc(no_inline)]
pub use fundamental_i18n_keys::{error_key, plain_placeholder_names, scan_template};

#[doc(no_inline)]
pub use fundamental_core::i18n::{
    Arg, ArgValue, Checked, Languages, Locale, LocaleParseError, Localized, NoopTranslator,
    Translator, TranslatorSource, current, interpolate, set_translator, set_translator_source,
    source_fingerprint, translate, translator_source, with,
};

/// Installs `translator` as the process-wide default.
pub fn install(translator: CatalogTranslator) {
    set_translator(Arc::new(translator));
}

/// Installs `catalog` as the process-wide source of a translator per locale.
///
/// ```
/// # extern crate fundamental_i18n;
/// use fundamental_i18n::{Catalog, Locale, install_catalog, m, translator_source, with};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let catalog = Catalog::builder("en".parse()?).build();
/// install_catalog(catalog);
///
/// let nl: Locale = "nl".parse()?;
/// let source = translator_source().expect("installed above");
/// let said = with(source.translator(&nl), || m::validate::required("E-mail").to_string());
/// assert_eq!(said, "E-mail is verplicht");
/// # Ok(())
/// # }
/// ```
pub fn install_catalog(catalog: Catalog) {
    set_translator_source(Arc::new(catalog));
}

fundamental_messages::messages!();
