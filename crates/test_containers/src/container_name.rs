//! The container names this harness can ask Docker for; no name may contain another.

/// Every container name this harness can ask Docker for.
///
/// Docker's `name` filter is an unanchored substring match, so no name may contain another;
/// `the_names_are_pairwise_non_containing` asserts it, and the numeric segments keep it open.
pub const ALL_CONTAINER_NAMES: &[&str] = &[
    ENGINE,
    NATS_01_ADAPTER,
    NATS_02_SYNC,
    NATS_03_PROJECTION,
    NATS_04_AUTHZ,
    NATS_05_AUTH,
    NATS_06_NKEY,
    NATS_07_TOKEN,
    OPENBAO_01_TRANSIT,
    OPENBAO_02_LEASES,
    OPENBAO_03_NATS,
    OPENBAO_04_KV,
    OPENBAO_05_PKI,
    ENGINE_AUTH,
    ENGINE_FAULT,
    NATS_08_FAULT,
    NATS_09_BENCH,
];

/// The one database node, shared by every suite that needs one.
///
/// One node for both wires; each test provisions its own database or keyspace on it.
pub const ENGINE: &str = "fundamental-test-engine";

/// NATS for `fundamental_nats`'s own adapter suite.
///
/// NATS servers are per binary: core NATS has no subject namespace a test can take.
pub const NATS_01_ADAPTER: &str = "fundamental-test-nats-01-adapter";

/// NATS for `fundamental_sync`'s change-bridge suite.
pub const NATS_02_SYNC: &str = "fundamental-test-nats-02-sync";

/// NATS for `fundamental_sync_server`'s projection suite.
pub const NATS_03_PROJECTION: &str = "fundamental-test-nats-03-projection";

/// NATS for `fundamental_authz`'s live-service suite.
pub const NATS_04_AUTHZ: &str = "fundamental-test-nats-04-authz";

/// NATS with authentication on, for the suites that prove a credential reaches the server.
///
/// Its own server, because a credential matters only against a server that requires one.
pub const NATS_05_AUTH: &str = "fundamental-test-nats-05-auth";

/// NATS whose one application user is an NKey, for the suite that proves a Transit key signs as
/// it; its configuration is written at run time.
pub const NATS_06_NKEY: &str = "fundamental-test-nats-06-nkey";

/// NATS that authenticates by one bearer token, for the suite that proves a token held in the
/// secret store reaches the server.
///
/// NATS authenticates by a token or by users, never both.
pub const NATS_07_TOKEN: &str = "fundamental-test-nats-07-token";

/// YugabyteDB with password authentication on, on both wires.
///
/// Its own node: a recipe change to the shared [`ENGINE`] would recreate it under every checkout.
pub const ENGINE_AUTH: &str = "fundamental-test-auth-engine";

/// YugabyteDB that a fault run restarts, configured as [`ENGINE`] is.
///
/// Its own node, because a restart fails every suite on the node.
pub const ENGINE_FAULT: &str = "fundamental-test-fault-engine";

/// NATS that a fault run stops and starts again.
pub const NATS_08_FAULT: &str = "fundamental-test-nats-08-fault";

/// NATS for `fundamental_bench`'s load and soak runs.
pub const NATS_09_BENCH: &str = "fundamental-test-nats-09-bench";

/// OpenBao, in development mode, for the suites that lease database credentials from it.
pub const OPENBAO_02_LEASES: &str = "fundamental-test-openbao-02-leases";

/// OpenBao, in development mode, for `fundamental_secrets`' Transit signing suite.
///
/// Per suite, because Transit key names are one flat namespace on a server.
pub const OPENBAO_01_TRANSIT: &str = "fundamental-test-openbao-01-transit";

/// OpenBao, in development mode, for `fundamental_nats`' suite of credentials read from the
/// secret store and NKeys signed there.
pub const OPENBAO_03_NATS: &str = "fundamental-test-openbao-03-nats";

/// OpenBao, in development mode, for `fundamental_secrets`' key-value write suite and the
/// crates that write secrets through it.
pub const OPENBAO_04_KV: &str = "fundamental-test-openbao-04-kv";
/// OpenBao, in development mode, for `fundamental_secrets`' PKI signing suite.
///
/// Its own server, because each process holds exactly one [`OpenBaoServer`](crate::openbao::OpenBaoServer).
pub const OPENBAO_05_PKI: &str = "fundamental-test-openbao-05-pki";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn the_names_are_pairwise_non_containing() {
        // The name filter is a substring match, so containment is attachment.
        for outer in ALL_CONTAINER_NAMES {
            for inner in ALL_CONTAINER_NAMES {
                if outer == inner {
                    continue;
                }
                assert!(
                    !outer.contains(inner),
                    "'{inner}' is a substring of '{outer}': a lookup for '{inner}' \
                     would find '{outer}'s container and silently share it"
                );
            }
        }
    }

    #[test]
    fn the_names_are_unique() {
        let mut seen = ALL_CONTAINER_NAMES.to_vec();
        seen.sort_unstable();
        let count = seen.len();
        seen.dedup();
        assert_eq!(
            count,
            seen.len(),
            "two suites are claiming one container name"
        );
    }

    #[test]
    fn every_name_is_swept_by_the_shared_prefix() {
        // The documented recovery filters `docker ps` by the `fundamental-test-` name.
        for name in ALL_CONTAINER_NAMES {
            assert!(name.starts_with("fundamental-test-"), "{name}");
        }
    }
}
