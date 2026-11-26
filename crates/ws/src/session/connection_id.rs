//! [`ConnectionId`]: the identifier of one WebSocket connection.

use fundamental_macros::Identifier;
use uuid::Uuid;

/// A unique id for a single WebSocket connection, minted when it is accepted.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Identifier)]
#[identifier(generate = "uuid_v4")]
pub struct ConnectionId(Uuid);

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::GeneratedId;

    #[test]
    fn generated_ids_are_unique() {
        assert_ne!(ConnectionId::generate(), ConnectionId::generate());
    }
}
