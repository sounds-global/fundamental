//! [`BrandTerm`], [`BrandTerms`] and [`install_brand_terms`]: the brand said by `{name :brand}`.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use fundamental_macros::Error;
use mf2_runtime::{ErrorSink, FnContext, FormatError, Function, Options, Sink, Value};

/// A word of the brand a message may say: the operand of `:brand`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BrandTerm {
    /// The brand's name, as a reader knows it: `{name :brand}`.
    Name,
    /// The name of the legal entity behind it: `{legal-name :brand}`.
    LegalName,
    /// The domain of its public site: `{site-domain :brand}`.
    SiteDomain,
    /// The domain of its platform: `{platform-domain :brand}`.
    PlatformDomain,
}

impl BrandTerm {
    /// Every term, in the order of the vocabulary.
    pub const ALL: [Self; 4] = [
        Self::Name,
        Self::LegalName,
        Self::SiteDomain,
        Self::PlatformDomain,
    ];

    /// The term a message names `operand`.
    #[must_use]
    pub fn named(operand: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|term| term.name() == operand)
    }

    /// Its name in a message: `legal-name`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::LegalName => "legal-name",
            Self::SiteDomain => "site-domain",
            Self::PlatformDomain => "platform-domain",
        }
    }
}

/// The brand's words, as the app's brand holds them: what `{name :brand}` says.
pub trait BrandTerms: Send + Sync {
    /// The brand's word for `term`.
    fn term(&self, term: BrandTerm) -> &str;
}

/// The brand is installed once per process.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BrandTermsInstalled {
    /// The process installed its brand terms before.
    #[error(
        code = "MESSAGES_001",
        message = "The brand terms are installed already; a process says one brand",
        status = 500
    )]
    Already,
}

static TERMS: OnceLock<Box<dyn BrandTerms>> = OnceLock::new();

static WARNED: AtomicBool = AtomicBool::new(false);

/// Installs `terms` as the brand every message's `:brand` says, once per process.
///
/// # Errors
/// [`BrandTermsInstalled::Already`] when the process installed its brand terms before.
pub fn install_brand_terms(terms: impl BrandTerms + 'static) -> Result<(), BrandTermsInstalled> {
    TERMS
        .set(Box::new(terms))
        .map_err(|_| BrandTermsInstalled::Already)
}

/// `:brand`: its operand's word from the installed brand terms, or a formatting error.
pub(crate) struct Brand;

pub(crate) static BRAND: Brand = Brand;

impl Function for Brand {
    fn resolve<'a>(
        &self,
        _cx: &FnContext<'_>,
        operand: Option<&Value<'a>>,
        _options: &Options<'_, 'a>,
        errs: &mut dyn ErrorSink,
    ) -> Option<Value<'a>> {
        let Some(term) = operand.and_then(Value::as_str).and_then(BrandTerm::named) else {
            errs.error(FormatError::BadOperand);
            return None;
        };
        let Some(terms) = TERMS.get() else {
            if !WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    term = term.name(),
                    "a message says the brand before any brand terms are installed"
                );
            }
            errs.error(FormatError::MessageFunctionError);
            return None;
        };
        Some(Value::Str(terms.term(term)))
    }

    fn format(&self, _cx: &FnContext<'_>, value: &Value<'_>, out: &mut dyn Sink) {
        if let Some(text) = value.as_str() {
            out.push_str(text);
        }
    }
}
