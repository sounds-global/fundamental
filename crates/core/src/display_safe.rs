/// Whether `character` could break a line or hide what a reader sees: Cc, a line or paragraph
/// separator, or an invisible format character (Cf).
#[must_use]
pub fn is_unsafe_for_display(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{00AD}'
                | '\u{061C}'
                | '\u{180E}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{206F}'
                | '\u{FEFF}'
                | '\u{FFF9}'..='\u{FFFB}'
        )
}

#[cfg(test)]
mod tests {
    use super::is_unsafe_for_display;

    #[test]
    fn line_breaks_bidi_overrides_and_zero_width_characters_are_unsafe() {
        for character in [
            '\r', '\n', '\0', '\t', '\u{7f}', '\u{85}', '\u{2028}', '\u{2029}', '\u{202e}',
            '\u{2066}', '\u{200b}', '\u{feff}',
        ] {
            assert!(is_unsafe_for_display(character), "{character:?}");
        }
        for character in ['a', 'é', 'ß', '€', ' ', '日'] {
            assert!(!is_unsafe_for_display(character), "{character:?}");
        }
    }
}
