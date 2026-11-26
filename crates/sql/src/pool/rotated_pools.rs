use sqlx::PgPool;

/// The connections a credential rotation replaced: the pool's own, and each live
/// reservation's, for the caller to drain and close.
#[derive(Debug)]
#[must_use = "replaced connections hold the old credential until they are closed"]
pub struct RotatedPools {
    primary: PgPool,
    reserved: Vec<PgPool>,
}

impl RotatedPools {
    pub(crate) fn new(primary: PgPool, reserved: Vec<PgPool>) -> Self {
        Self { primary, reserved }
    }

    /// The pool's own replaced connections.
    #[must_use]
    pub fn primary(&self) -> &PgPool {
        &self.primary
    }

    /// How many reservations were rebuilt on the fresh credential.
    #[must_use]
    pub fn reserved(&self) -> usize {
        self.reserved.len()
    }

    /// Every replaced pool, the primary first.
    pub fn into_pools(self) -> Vec<PgPool> {
        let mut pools = Vec::with_capacity(1 + self.reserved.len());
        pools.push(self.primary);
        pools.extend(self.reserved);
        pools
    }
}
