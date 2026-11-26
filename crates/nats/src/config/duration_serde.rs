//! (De)serializes a [`Duration`] as `"100ms"`, `"5s"` or `"1m"`, or a bare integer of
//! milliseconds.

use std::time::Duration;

use serde::{Deserialize, Deserializer, Serializer};

pub(super) fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&format!("{}ms", duration.as_millis()))
}

pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let parse = |raw: &str, unit_ms: u64| {
        raw.parse::<u64>()
            .map(|n| Duration::from_millis(n * unit_ms))
            .map_err(serde::de::Error::custom)
    };
    if let Some(ms) = s.strip_suffix("ms") {
        parse(ms, 1)
    } else if let Some(secs) = s.strip_suffix('s') {
        parse(secs, 1_000)
    } else if let Some(mins) = s.strip_suffix('m') {
        parse(mins, 60_000)
    } else {
        parse(&s, 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    struct Wrap(#[serde(with = "super")] Duration);

    fn roundtrip(input: &str) -> Duration {
        serde_json::from_str::<Wrap>(&format!("\"{input}\""))
            .unwrap()
            .0
    }

    #[test]
    fn parses_units() {
        assert_eq!(roundtrip("250ms"), Duration::from_millis(250));
        assert_eq!(roundtrip("5s"), Duration::from_secs(5));
        assert_eq!(roundtrip("2m"), Duration::from_secs(120));
        assert_eq!(roundtrip("400"), Duration::from_millis(400));
    }

    #[test]
    fn serializes_as_millis() {
        let json = serde_json::to_string(&Wrap(Duration::from_secs(3))).unwrap();
        assert_eq!(json, "\"3000ms\"");
    }

    #[test]
    fn rejects_garbage() {
        assert!(serde_json::from_str::<Wrap>("\"abc\"").is_err());
    }
}
