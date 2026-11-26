/// The island bootstrap's exact text, which a Content Security Policy hash is computed over.
#[must_use]
pub fn hydration_bootstrap() -> &'static str {
    include_str!("hydration_bootstrap.js")
}
