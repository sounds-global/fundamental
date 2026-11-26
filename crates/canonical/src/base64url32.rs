use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// The one unpadded base64url spelling of 32 bytes.
pub(crate) fn encode(bytes: &[u8; 32]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Read 32 bytes back from their one spelling.
///
/// The result is rendered again and must equal the input, refusing non-zero padding bits.
pub(crate) fn decode(text: &str) -> Option<[u8; 32]> {
    if text.len() != 43 {
        return None;
    }
    let bytes: [u8; 32] = URL_SAFE_NO_PAD.decode(text).ok()?.try_into().ok()?;
    (encode(&bytes) == text).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};

    #[test]
    fn thirty_two_bytes_round_trip_in_forty_three_characters() {
        let bytes = [0xA5_u8; 32];
        let text = encode(&bytes);
        assert_eq!(text.len(), 43);
        assert_eq!(decode(&text), Some(bytes));
    }

    #[test]
    fn every_second_spelling_is_refused() {
        let text = encode(&[0_u8; 32]);
        let mut padding_bits = text.clone();
        padding_bits.pop();
        padding_bits.push('B');
        for other in [
            format!("{text}="),
            text.replace('A', "+"),
            padding_bits,
            text[..42].to_owned(),
            format!(" {text}"),
        ] {
            assert_eq!(decode(&other), None, "{other} was read");
        }
    }
}
