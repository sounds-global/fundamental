//! [`AuthzWorkers`]: the NATS queue group the authorization responders share.

use fundamental_nats::Queue;

/// The queue group every authorization RPC responder joins.
///
/// Load-balances each subject across replicas; every `#[respond]` handler names this type, so
/// a misspelled group is a compile error.
#[derive(Queue)]
#[queue(name = "authz_workers")]
pub struct AuthzWorkers;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_the_group_name() {
        assert_eq!(<AuthzWorkers as Queue>::NAME, "authz_workers");
    }
}
