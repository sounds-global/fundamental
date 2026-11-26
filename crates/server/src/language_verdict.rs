use fundamental_core::i18n::Locale;

use crate::ReturnPath;

/// What [`LocaleChoice`](crate::LocaleChoice) does with one request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LanguageVerdict {
    /// Nothing to record; serve the request unchanged. Every refusal fails open here.
    Untouched,
    /// A person chose a language this surface serves: record it and send them back to `return_to`.
    Record {
        /// The language to write into the cookie.
        language: Locale,
        /// Where to send them, rebuilt from the matched route without the parameter.
        return_to: ReturnPath,
    },
    /// The request carried a language and the caller is not a person: serve it.
    Ignored,
    /// Another site put the choice in the link: send the person to `return_to`, recording nothing.
    Declined {
        /// The same page, without the choice.
        return_to: ReturnPath,
    },
}
