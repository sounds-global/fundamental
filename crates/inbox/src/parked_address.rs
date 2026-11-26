//! [`ParkedAddress`] — where one parked record is, as an operator types it.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, SecondsFormat, Utc};

use crate::{EffectName, InboxError};

/// A parked record's key, written `<effect>/<stream>/<stream created at>/<sequence>`.
///
/// No part may contain a `/`, so the four parts cannot be confused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParkedAddress {
    /// The effect it was delivered to.
    pub effect: String,
    /// The stream it sits in.
    pub stream: String,
    /// When that stream was created.
    pub stream_created_at: DateTime<Utc>,
    /// Its sequence in that stream.
    pub stream_sequence: u64,
}

impl fmt::Display for ParkedAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}/{}/{}",
            self.effect,
            self.stream,
            self.stream_created_at
                .to_rfc3339_opts(SecondsFormat::AutoSi, true),
            self.stream_sequence
        )
    }
}

impl FromStr for ParkedAddress {
    type Err = InboxError;

    fn from_str(address: &str) -> Result<Self, Self::Err> {
        let refuse = |detail: &str| InboxError::InvalidParkedAddress {
            address: address.to_owned(),
            detail: detail.to_owned(),
        };
        let parts: Vec<&str> = address.split('/').collect();
        let [effect, stream, created, sequence] = parts.as_slice() else {
            return Err(refuse("four parts separated by `/` are required"));
        };
        if !EffectName::is_valid(effect) {
            return Err(refuse("the effect is not an effect name"));
        }
        if stream.is_empty() {
            return Err(refuse("the stream is empty"));
        }
        let stream_created_at = DateTime::parse_from_rfc3339(created)
            .map_err(|_| refuse("the stream's creation time is not RFC 3339"))?
            .with_timezone(&Utc);
        let stream_sequence = sequence
            .parse()
            .map_err(|_| refuse("the sequence is not a number"))?;
        Ok(Self {
            effect: (*effect).to_owned(),
            stream: (*stream).to_owned(),
            stream_created_at,
            stream_sequence,
        })
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn an_address_round_trips_through_its_text() {
        let address = ParkedAddress {
            effect: "billing.book-deposit".to_owned(),
            stream: "DEPOSITS".to_owned(),
            stream_created_at: DateTime::parse_from_rfc3339("2026-09-30T12:00:00.123456Z")
                .expect("time")
                .with_timezone(&Utc),
            stream_sequence: 42,
        };
        let text = address.to_string();
        assert_eq!(
            text,
            "billing.book-deposit/DEPOSITS/2026-09-30T12:00:00.123456Z/42"
        );
        assert_eq!(text.parse::<ParkedAddress>().expect("parses"), address);
    }

    #[test]
    fn every_malformed_address_is_refused_with_its_code() {
        for text in [
            "billing/DEPOSITS/2026-09-30T12:00:00Z",
            "Billing/DEPOSITS/2026-09-30T12:00:00Z/1",
            "billing//2026-09-30T12:00:00Z/1",
            "billing/DEPOSITS/yesterday/1",
            "billing/DEPOSITS/2026-09-30T12:00:00Z/-1",
        ] {
            let error = text.parse::<ParkedAddress>().expect_err(text);
            assert_eq!(error.error_code(), "SINBOX_014", "{text}");
        }
    }
}
