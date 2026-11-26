//! Typed axes of variation.

mod device;
mod platform;
#[allow(clippy::module_inception)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
mod variant;

pub use device::Device;
pub use platform::Platform;
pub use variant::Variant;
