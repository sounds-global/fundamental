use crate::{CanonicalError, Limits, Profile};
use std::borrow::Cow;

/// The largest integer every JSON implementation represents exactly (I-JSON, RFC 7493 §2.2).
const MAX_SAFE_INTEGER: i64 = (1 << 53) - 1;

/// A cursor over bytes that must already be in RFC 8785 canonical form.
pub(crate) struct Reader<'de> {
    input: &'de [u8],
    pos: usize,
    limits: Limits,
    profile: Profile,
    depth: u32,
    values: u32,
    #[cfg(test)]
    compared: u64,
}

impl<'de> Reader<'de> {
    /// Refuse an oversized input or a byte order mark before reading anything.
    pub(crate) fn new(
        input: &'de [u8],
        limits: Limits,
        profile: Profile,
    ) -> Result<Self, CanonicalError> {
        limits.admit_len(input.len())?;
        if input.starts_with(&[0xEF, 0xBB, 0xBF]) {
            return Err(CanonicalError::ByteOrderMark);
        }
        Ok(Self {
            input,
            pos: 0,
            limits,
            profile,
            depth: 0,
            values: 0,
            #[cfg(test)]
            compared: 0,
        })
    }

    pub(crate) const fn offset(&self) -> usize {
        self.pos
    }

    pub(crate) fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    pub(crate) const fn limits(&self) -> &Limits {
        &self.limits
    }

    /// The error for whatever sits at the cursor when the grammar wanted `expected`.
    pub(crate) fn unexpected(&self, expected: &'static str) -> CanonicalError {
        let offset = self.pos;
        match self.peek() {
            Some(b' ' | b'\t' | b'\n' | b'\r') => CanonicalError::Whitespace { offset },
            Some(b'/') => CanonicalError::Comment { offset },
            _ => CanonicalError::Syntax { offset, expected },
        }
    }

    pub(crate) fn expect(
        &mut self,
        byte: u8,
        expected: &'static str,
    ) -> Result<(), CanonicalError> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.unexpected(expected))
        }
    }

    /// Count a value about to start, before any work on it.
    pub(crate) fn begin_value(&mut self) -> Result<(), CanonicalError> {
        self.values += 1;
        if self.values > self.limits.max_values {
            return Err(CanonicalError::TooManyValues {
                offset: self.pos,
                max: self.limits.max_values,
            });
        }
        Ok(())
    }

    /// Enter a container, refusing before it opens if that would exceed the depth.
    pub(crate) fn enter(&mut self) -> Result<(), CanonicalError> {
        self.depth += 1;
        if self.depth > self.limits.max_depth {
            return Err(CanonicalError::TooDeep {
                offset: self.pos,
                max: self.limits.max_depth,
            });
        }
        Ok(())
    }

    /// Record key bytes compared, so a test can bound the reader's total work.
    #[cfg(test)]
    pub(crate) fn note_compared(&mut self, bytes: usize) {
        self.compared += bytes as u64;
    }

    /// Bytes advanced plus key bytes compared: the reader's total work so far.
    #[cfg(test)]
    pub(crate) fn work(&self) -> u64 {
        self.pos as u64 + self.compared
    }

    pub(crate) fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// Refuse anything after the one top-level value.
    pub(crate) fn finish(&self) -> Result<(), CanonicalError> {
        match self.peek() {
            None => Ok(()),
            Some(b' ' | b'\t' | b'\n' | b'\r') => {
                Err(CanonicalError::Whitespace { offset: self.pos })
            }
            Some(_) => Err(CanonicalError::TrailingBytes { offset: self.pos }),
        }
    }

    /// Consume `true`, `false` or `null`, spelled exactly.
    pub(crate) fn literal(
        &mut self,
        word: &'static [u8],
        expected: &'static str,
    ) -> Result<(), CanonicalError> {
        let end = self.pos + word.len();
        if self.input.get(self.pos..end) == Some(word) {
            self.pos = end;
            Ok(())
        } else {
            Err(CanonicalError::Syntax {
                offset: self.pos,
                expected,
            })
        }
    }

    /// Consume an integer in its one canonical spelling, within ±(2⁵³−1).
    pub(crate) fn integer(&mut self) -> Result<i64, CanonicalError> {
        let start = self.pos;
        let negative = self.peek() == Some(b'-');
        if negative {
            self.pos += 1;
        }
        let digits_start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        let digits = &self.input[digits_start..self.pos];
        if digits.is_empty() {
            return Err(CanonicalError::Syntax {
                offset: self.pos,
                expected: "a digit",
            });
        }
        if matches!(self.peek(), Some(b'.' | b'e' | b'E')) {
            return Err(CanonicalError::Float { offset: start });
        }
        let leading_zero = digits.len() > 1 && digits[0] == b'0';
        let negative_zero = negative && digits == b"0";
        if leading_zero || negative_zero {
            return Err(CanonicalError::NonCanonicalInteger { offset: start });
        }
        // Sixteen digits already exceed 2⁵³−1, so accumulation never overflows an i64.
        if digits.len() > 16 {
            return Err(CanonicalError::IntegerOutOfRange { offset: start });
        }
        let magnitude = digits
            .iter()
            .fold(0_i64, |value, digit| value * 10 + i64::from(digit - b'0'));
        if magnitude > MAX_SAFE_INTEGER {
            return Err(CanonicalError::IntegerOutOfRange { offset: start });
        }
        Ok(if negative { -magnitude } else { magnitude })
    }

    /// Consume a string whose every character is written the way RFC 8785 writes it.
    pub(crate) fn string(&mut self) -> Result<Cow<'de, str>, CanonicalError> {
        let start = self.pos;
        self.expect(b'"', "a string")?;
        let content_start = self.pos;
        let mut escaped = false;
        loop {
            if self.pos - content_start > self.limits.max_string_bytes {
                return Err(CanonicalError::StringTooLong {
                    offset: start,
                    max: self.limits.max_string_bytes,
                });
            }
            match self.peek() {
                None => {
                    return Err(CanonicalError::Syntax {
                        offset: self.pos,
                        expected: "a closing quote",
                    });
                }
                Some(b'"') => break,
                Some(b'\\') => {
                    escaped = true;
                    self.escape()?;
                }
                Some(0x00..=0x1F) => {
                    return Err(CanonicalError::UnescapedControl { offset: self.pos });
                }
                Some(_) => self.pos += 1,
            }
        }
        let raw = &self.input[content_start..self.pos];
        self.pos += 1;
        let text = core::str::from_utf8(raw).map_err(|error| CanonicalError::InvalidUtf8 {
            offset: content_start + error.valid_up_to(),
        })?;
        let value = if escaped {
            Cow::Owned(unescape(text))
        } else {
            Cow::Borrowed(text)
        };
        self.check_profile(&value, start)?;
        Ok(value)
    }

    /// Accept exactly the escapes RFC 8785 writes.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn escape(&mut self) -> Result<(), CanonicalError> {
        let offset = self.pos;
        match self.input.get(self.pos + 1).copied() {
            Some(b'"' | b'\\' | b'b' | b't' | b'n' | b'f' | b'r') => {
                self.pos += 2;
                Ok(())
            }
            Some(b'u') => {
                let unit = self.hex4(self.pos + 2)?;
                if (0xD800..=0xDFFF).contains(&unit) {
                    let paired = (0xD800..=0xDBFF).contains(&unit)
                        && self.input.get(self.pos + 6..self.pos + 8) == Some(b"\\u")
                        && self
                            .hex4(self.pos + 8)
                            .is_ok_and(|low| (0xDC00..=0xDFFF).contains(&low));
                    return Err(if paired {
                        CanonicalError::ForbiddenEscape { offset }
                    } else {
                        CanonicalError::LoneSurrogate { offset }
                    });
                }
                let short_form = matches!(unit, 0x08 | 0x09 | 0x0A | 0x0C | 0x0D);
                let lower_case = self.input[self.pos + 2..self.pos + 6]
                    .iter()
                    .all(|byte| !byte.is_ascii_uppercase());
                if unit < 0x20 && !short_form && lower_case {
                    self.pos += 6;
                    Ok(())
                } else {
                    Err(CanonicalError::ForbiddenEscape { offset })
                }
            }
            Some(_) => Err(CanonicalError::ForbiddenEscape { offset }),
            None => Err(CanonicalError::Syntax {
                offset,
                expected: "an escape",
            }),
        }
    }

    fn hex4(&self, at: usize) -> Result<u32, CanonicalError> {
        let digits = self.input.get(at..at + 4).ok_or(CanonicalError::Syntax {
            offset: at,
            expected: "four hex digits",
        })?;
        digits.iter().try_fold(0_u32, |value, byte| {
            let nibble = char::from(*byte)
                .to_digit(16)
                .ok_or(CanonicalError::Syntax {
                    offset: at,
                    expected: "four hex digits",
                })?;
            Ok(value << 4 | nibble)
        })
    }

    fn check_profile(&self, text: &str, offset: usize) -> Result<(), CanonicalError> {
        #[cfg(feature = "registry")]
        if self.profile == Profile::Registry {
            return registry_check(text, offset);
        }
        #[cfg(not(feature = "registry"))]
        let _ = (text, offset, self.profile);
        Ok(())
    }
}

/// Decode an escaped string the reader has already proved holds only canonical escapes.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match chars.next() {
            Some('b') => out.push('\u{08}'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('f') => out.push('\u{0C}'),
            Some('r') => out.push('\r'),
            Some('u') => {
                let code = chars
                    .by_ref()
                    .take(4)
                    .filter_map(|digit| digit.to_digit(16))
                    .fold(0_u32, |value, nibble| value << 4 | nibble);
                // `escape` admitted only U+0000–U+001F here, every one a valid scalar.
                if let Some(control) = char::from_u32(code) {
                    out.push(control);
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

#[cfg(feature = "registry")]
fn registry_check(text: &str, offset: usize) -> Result<(), CanonicalError> {
    use icu_normalizer::ComposingNormalizerBorrowed;
    use icu_properties::CodePointMapData;
    use icu_properties::props::GeneralCategory;

    if !ComposingNormalizerBorrowed::new_nfc().is_normalized(text) {
        return Err(CanonicalError::NotNfc { offset });
    }
    let categories = CodePointMapData::<GeneralCategory>::new();
    if text
        .chars()
        .any(|character| categories.get(character) == GeneralCategory::Unassigned)
    {
        return Err(CanonicalError::Unassigned { offset });
    }
    Ok(())
}
