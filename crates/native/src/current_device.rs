use fundamental_core::Device;

use crate::current_platform;

/// The [`Device`] this binary was compiled for.
#[must_use]
pub fn current_device() -> Device {
    current_platform().device()
}

#[cfg(test)]
mod tests {
    use super::current_device;
    use fundamental_core::Device;

    #[test]
    fn current_device_is_desktop_on_the_test_host() {
        assert_eq!(current_device(), Device::Desktop);
    }
}
