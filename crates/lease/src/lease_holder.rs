use std::fmt;

use uuid::Uuid;

/// The process holding a lease: 122 random bits, drawn once per [`Lease`](crate::Lease).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LeaseHolder(Uuid);

impl LeaseHolder {
    pub(crate) fn generate() -> Self {
        Self(Uuid::new_v4())
    }

    /// The holder as the UUID its lease row records.
    #[must_use]
    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for LeaseHolder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn two_leases_never_share_a_holder() {
        let holders: HashSet<LeaseHolder> = (0..10_000).map(|_| LeaseHolder::generate()).collect();
        assert_eq!(holders.len(), 10_000);
    }
}
