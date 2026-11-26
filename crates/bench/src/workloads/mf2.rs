//! Messages said in a reader's language: MessageFormat 2 with plurals and numbers.

use fundamental_messages_fixture::m;

/// A plain message, a number, a plural and a selection in English and Dutch; their total length.
#[must_use]
pub fn format() -> usize {
    let mut said = 0;
    for language in ["en", "nl"] {
        said += m::panel::save_for_owner().format_in(language).len();
        said += m::panel::version(12).format_in(language).len();
        said += m::panel::pages(1).format_in(language).len();
        said += m::panel::pages(7).format_in(language).len();
        said += m::panel::filter("stage").format_in(language).len();
        said += m::panel::greeting("Ada").format_in(language).len();
    }
    said
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_is_said() {
        assert!(format() > 60);
    }
}
