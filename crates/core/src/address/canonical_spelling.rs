//! [`canonical_spelling`] — a path in the one spelling every address uses.

#[cfg(not(feature = "std"))]
use alloc::{borrow::Cow, string::String};
#[cfg(feature = "std")]
use std::borrow::Cow;

/// `path` in its one spelling: no empty segment, every percent-escape in upper case.
pub(crate) fn canonical_spelling(path: &str) -> Cow<'_, str> {
    let has_empty_segment = path.len() > 1 && path[1..].split('/').any(str::is_empty);
    let has_lower_escape = path
        .as_bytes()
        .windows(3)
        .any(|w| w[0] == b'%' && (w[1].is_ascii_lowercase() || w[2].is_ascii_lowercase()));
    if !has_empty_segment && !has_lower_escape {
        return Cow::Borrowed(path);
    }
    let mut spelled = String::with_capacity(path.len());
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        spelled.push('/');
        let mut escape_left = 0;
        for ch in segment.chars() {
            match (ch, escape_left) {
                ('%', _) => {
                    spelled.push('%');
                    escape_left = 2;
                }
                (hex, 1..=2) if hex.is_ascii_hexdigit() => {
                    spelled.push(hex.to_ascii_uppercase());
                    escape_left -= 1;
                }
                (other, _) => {
                    spelled.push(other);
                    escape_left = 0;
                }
            }
        }
    }
    if spelled.is_empty() {
        spelled.push('/');
    }
    Cow::Owned(spelled)
}
