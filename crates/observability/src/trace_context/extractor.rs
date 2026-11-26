/// A key/value carrier a trace context is read from, free of any OpenTelemetry dependency.
pub trait Extractor {
    /// Returns the value stored under `key`, if any.
    fn get(&self, key: &str) -> Option<&str>;

    /// Returns every key present in the carrier.
    fn keys(&self) -> Vec<&str>;
}
