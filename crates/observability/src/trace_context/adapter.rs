//! Newtypes adapting the framework's carrier traits to OpenTelemetry's.

use super::Injector;

/// Presents a framework [`Injector`] as an `opentelemetry` one.
pub(super) struct InjectorAdapter<'a, C: ?Sized>(pub(super) &'a mut C);

impl<C: Injector + ?Sized> opentelemetry::propagation::Injector for InjectorAdapter<'_, C> {
    fn set(&mut self, key: &str, value: String) {
        self.0.set(key, value);
    }
}
