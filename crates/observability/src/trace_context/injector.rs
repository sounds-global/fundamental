/// A key/value carrier a trace context is written into, free of any OpenTelemetry dependency.
pub trait Injector {
    /// Writes `value` under `key`, replacing any previous value.
    fn set(&mut self, key: &str, value: String);
}
