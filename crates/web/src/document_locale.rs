use fundamental_core::i18n::Locale;
use wasm_bindgen::JsCast;

/// The language of the document the browser is hydrating, from `<html lang="…">`.
///
/// Browser only; `None` when there is no document or the attribute holds no valid tag.
///
/// ```text
/// pub fn live_boot() {
///     if let Some(locale) = fundamental::web::document_locale() {
///         fundamental::i18n::install(catalog().translator(&locale));
///     }
/// }
/// ```
/// ```rust,no_run
/// # extern crate fundamental_web;
/// use fundamental_web::document_locale;
///
/// if let Some(locale) = document_locale() {
///     // Install a translator for it — see `fundamental_i18n::install`.
///     let _ = locale.language();
/// }
/// ```
#[must_use]
pub fn document_locale() -> Option<Locale> {
    let element = web_sys::window()?.document()?.document_element()?;
    let lang = element.dyn_into::<web_sys::HtmlElement>().ok()?.lang();
    let lang = lang.trim();
    if lang.is_empty() {
        return None;
    }
    lang.parse().ok()
}
