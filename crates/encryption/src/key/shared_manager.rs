use std::sync::Arc;

use crate::KeyManager;

/// A shared `KeyManager`.
pub type SharedKeyManager = Arc<KeyManager>;
