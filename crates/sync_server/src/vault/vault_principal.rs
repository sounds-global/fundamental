use fundamental_keys::VaultOwnerId;

/// The blinded owner id a vault connection is bound to after an ownership proof, and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VaultPrincipal {
    owner_id: VaultOwnerId,
}

impl VaultPrincipal {
    /// Binds a connection to `owner_id` (called only after [`authenticate_vault`]).
    ///
    /// [`authenticate_vault`]: crate::authenticate_vault
    #[must_use]
    pub fn new(owner_id: VaultOwnerId) -> Self {
        Self { owner_id }
    }

    /// The blinded owner this connection may act as.
    #[must_use]
    pub fn owner_id(&self) -> &VaultOwnerId {
        &self.owner_id
    }
}
