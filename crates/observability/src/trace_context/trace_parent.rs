use std::fmt;

use super::{Extractor, TraceParentRejection};

const HEADER: &str = "traceparent";

/// Version `00` is 55 bytes; a later version may append fields, never kilobytes.
const MAX_LEN: usize = 256;

/// A version `00` value's length, and the prefix every later version shares.
const V00_LEN: usize = 55;

/// A W3C Trace Context `traceparent`: a trace id, a parent span id and the sampled flag.
///
/// Parsed here because the OpenTelemetry propagator accepts ids of the wrong length and signed
/// fields.
///
/// ```rust
/// # extern crate fundamental_observability;
/// use fundamental_observability::trace_context::TraceParent;
///
/// let parent = TraceParent::parse("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")?;
/// assert!(parent.is_sampled());
/// assert_eq!(
///     parent.to_string(),
///     "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
/// );
/// # Ok::<(), fundamental_observability::trace_context::TraceParentRejection>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceParent {
    trace_id: [u8; 16],
    parent_id: [u8; 8],
    sampled: bool,
}

impl TraceParent {
    /// Parses a `traceparent` value.
    /// Parses a `traceparent` value, strictly, keeping only the sampled flag.
    ///
    /// # Errors
    ///
    /// A [`TraceParentRejection`] naming the class of the failure, never the value.
    pub fn parse(value: &str) -> Result<Self, TraceParentRejection> {
        let bytes = value.as_bytes();
        if bytes.len() > MAX_LEN {
            return Err(TraceParentRejection::Oversized);
        }
        if bytes.len() < V00_LEN || bytes[2] != b'-' || bytes[35] != b'-' || bytes[52] != b'-' {
            return Err(TraceParentRejection::Malformed);
        }
        let version = hex_bytes::<1>(&bytes[0..2])?[0];
        match version {
            0xff => return Err(TraceParentRejection::UnsupportedVersion),
            0x00 if bytes.len() != V00_LEN => return Err(TraceParentRejection::Malformed),
            _ if bytes.len() > V00_LEN && bytes[V00_LEN] != b'-' => {
                return Err(TraceParentRejection::Malformed);
            }
            _ => {}
        }
        let trace_id = hex_bytes::<16>(&bytes[3..35])?;
        let parent_id = hex_bytes::<8>(&bytes[36..52])?;
        let flags = hex_bytes::<1>(&bytes[53..55])?[0];
        if trace_id == [0; 16] {
            return Err(TraceParentRejection::ZeroTraceId);
        }
        if parent_id == [0; 8] {
            return Err(TraceParentRejection::ZeroParentId);
        }
        Ok(Self {
            trace_id,
            parent_id,
            sampled: flags & 0x01 == 0x01,
        })
    }

    /// Reads and parses the `traceparent` a carrier holds.
    /// Reads and parses a carrier's `traceparent`; `Ok(None)` when it holds none.
    ///
    /// `tracestate` and baggage are never read.
    ///
    /// # Errors
    ///
    /// The [`TraceParentRejection`] of a value that is present and not valid.
    pub fn from_carrier<C>(carrier: &C) -> Result<Option<Self>, TraceParentRejection>
    where
        C: Extractor + ?Sized,
    {
        carrier.get(HEADER).map(Self::parse).transpose()
    }

    /// The 16-byte trace id.
    #[must_use]
    pub const fn trace_id(&self) -> [u8; 16] {
        self.trace_id
    }

    /// The 8-byte id of the sender's span, which becomes this span's parent.
    #[must_use]
    pub const fn parent_id(&self) -> [u8; 8] {
        self.parent_id
    }

    /// Whether the sender recorded its span.
    #[must_use]
    pub const fn is_sampled(&self) -> bool {
        self.sampled
    }
}

impl fmt::Display for TraceParent {
    /// The version `00` form, whatever version it was read from.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("00-")?;
        write_hex(f, &self.trace_id)?;
        f.write_str("-")?;
        write_hex(f, &self.parent_id)?;
        f.write_str(if self.sampled { "-01" } else { "-00" })
    }
}

fn hex_bytes<const N: usize>(digits: &[u8]) -> Result<[u8; N], TraceParentRejection> {
    if digits.len() != 2 * N {
        return Err(TraceParentRejection::Malformed);
    }
    let mut out = [0u8; N];
    for (byte, pair) in out.iter_mut().zip(digits.chunks_exact(2)) {
        *byte = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Ok(out)
}

/// One lowercase hex digit; uppercase is refused so one context has one spelling.
fn nibble(digit: u8) -> Result<u8, TraceParentRejection> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        _ => Err(TraceParentRejection::Malformed),
    }
}

fn write_hex(f: &mut fmt::Formatter<'_>, bytes: &[u8]) -> fmt::Result {
    bytes.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

    #[test]
    fn a_version_00_context_parses_and_round_trips() {
        let parent = TraceParent::parse(VALID).expect("the specification's own example");
        assert!(parent.is_sampled());
        assert_eq!(parent.trace_id()[0], 0x4b);
        assert_eq!(parent.parent_id()[7], 0xb7);
        assert_eq!(parent.to_string(), VALID);
    }

    #[test]
    fn only_the_sampled_bit_of_the_flags_is_kept() {
        let unsampled = VALID.replace("-01", "-00");
        assert!(
            !TraceParent::parse(&unsampled)
                .expect("unsampled")
                .is_sampled()
        );
        let other_bits = VALID.replace("-01", "-fe");
        let parsed = TraceParent::parse(&other_bits).expect("unknown bits are ignored");
        assert!(!parsed.is_sampled());
        assert!(parsed.to_string().ends_with("-00"));
    }

    #[test]
    fn a_later_version_is_read_by_its_first_fields() {
        let later = format!("cc{}-what-the-future-appends", &VALID[2..]);
        let parsed = TraceParent::parse(&later).expect("forward compatible");
        assert_eq!(parsed.to_string(), VALID);
    }

    #[test]
    fn a_later_version_must_continue_with_a_dash() {
        let glued = format!("cc{}x", &VALID[2..]);
        assert_eq!(
            TraceParent::parse(&glued),
            Err(TraceParentRejection::Malformed)
        );
    }

    #[test]
    fn version_00_with_anything_appended_is_malformed() {
        assert_eq!(
            TraceParent::parse(&format!("{VALID}-extra")),
            Err(TraceParentRejection::Malformed)
        );
    }

    #[test]
    fn version_ff_is_refused() {
        let reserved = format!("ff{}", &VALID[2..]);
        assert_eq!(
            TraceParent::parse(&reserved),
            Err(TraceParentRejection::UnsupportedVersion)
        );
    }

    #[test]
    fn wrong_lengths_signs_and_case_are_malformed() {
        let cases = [
            "00-4bf92f3577b34da6a3ce929d0e0e473-00f067aa0ba902b7-01",
            "00-+bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
            "0A-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-1",
            "00_4bf92f3577b34da6a3ce929d0e0e4736_00f067aa0ba902b7_01",
            " 00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "",
            "jan@example.nl",
        ];
        for case in cases {
            assert_eq!(
                TraceParent::parse(case),
                Err(TraceParentRejection::Malformed),
                "{case:?}"
            );
        }
    }

    #[test]
    fn zero_ids_are_refused() {
        let zero_trace = "00-00000000000000000000000000000000-00f067aa0ba902b7-01";
        let zero_parent = "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01";
        assert_eq!(
            TraceParent::parse(zero_trace),
            Err(TraceParentRejection::ZeroTraceId)
        );
        assert_eq!(
            TraceParent::parse(zero_parent),
            Err(TraceParentRejection::ZeroParentId)
        );
    }

    #[test]
    fn an_oversized_value_is_refused_before_it_is_read() {
        let oversized = format!("cc{}-{}", &VALID[2..], "a".repeat(MAX_LEN));
        assert_eq!(
            TraceParent::parse(&oversized),
            Err(TraceParentRejection::Oversized)
        );
    }

    #[test]
    fn a_non_ascii_value_is_malformed_without_panicking() {
        let multibyte = "é".repeat(30);
        assert_eq!(
            TraceParent::parse(&multibyte),
            Err(TraceParentRejection::Malformed)
        );
    }

    struct Carrier(Vec<(&'static str, &'static str)>);

    impl Extractor for Carrier {
        fn get(&self, key: &str) -> Option<&str> {
            self.0.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
        }

        fn keys(&self) -> Vec<&str> {
            self.0.iter().map(|(k, _)| *k).collect()
        }
    }

    #[test]
    fn a_carrier_without_the_header_is_not_a_rejection() {
        assert_eq!(TraceParent::from_carrier(&Carrier(Vec::new())), Ok(None));
    }

    #[test]
    fn a_carrier_is_read_for_traceparent_alone() {
        let carrier = Carrier(vec![
            ("traceparent", VALID),
            ("tracestate", "x=jan@example.nl"),
        ]);
        let parsed = TraceParent::from_carrier(&carrier).expect("valid");
        assert_eq!(parsed.map(|p| p.to_string()).as_deref(), Some(VALID));
    }

    #[test]
    fn a_carrier_with_a_hostile_value_reports_its_class() {
        let carrier = Carrier(vec![("traceparent", "jan@example.nl")]);
        assert_eq!(
            TraceParent::from_carrier(&carrier),
            Err(TraceParentRejection::Malformed)
        );
    }
}
