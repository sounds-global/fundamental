//! [`VersionedRequest`] — the two fields every inbound sync frame carries.

use crate::wire::SUPPORTED_VERSIONS;

/// An inbound wire request, readable as a protocol version and a correlation id
/// without knowing which request it is.
pub trait VersionedRequest {
    /// The wire protocol version the sender claims to speak.
    fn protocol_version(&self) -> u32;

    /// The id correlating this request with its reply.
    fn request_id(&self) -> u64;

    /// Whether [`SUPPORTED_VERSIONS`] holds the claimed version.
    fn version_is_supported(&self) -> bool {
        SUPPORTED_VERSIONS.contains(&self.protocol_version())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::PROTOCOL_VERSION;

    struct Frame(u32);

    impl VersionedRequest for Frame {
        fn protocol_version(&self) -> u32 {
            self.0
        }
        fn request_id(&self) -> u64 {
            1
        }
    }

    #[test]
    fn the_version_this_build_speaks_is_supported() {
        assert!(Frame(PROTOCOL_VERSION).version_is_supported());
    }

    #[test]
    fn a_version_outside_the_set_is_refused() {
        for version in [0, PROTOCOL_VERSION + 1, u32::MAX] {
            assert!(
                !Frame(version).version_is_supported(),
                "version {version} is not in {SUPPORTED_VERSIONS:?} and must be refused"
            );
        }
    }

    #[test]
    fn the_supported_set_contains_the_current_version() {
        assert!(SUPPORTED_VERSIONS.contains(&PROTOCOL_VERSION));
    }
}
