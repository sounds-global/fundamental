use std::collections::HashMap;

use zeroize::Zeroizing;

use crate::{EncryptionAlgorithm, KeyVersion};

#[derive(Debug, Clone)]
pub(crate) struct KeyEntry {
    pub(crate) id: String,
    pub(crate) versions: HashMap<u32, KeyVersion>,
    pub(crate) current_version: u32,
    /// The purpose this key was declared for, bound into every value sealed under it.
    pub(crate) purpose: String,
}

impl KeyEntry {
    pub(crate) fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            versions: HashMap::new(),
            current_version: 0,
            purpose: String::new(),
        }
    }

    pub(crate) fn add_version(
        &mut self,
        version: u32,
        material: Vec<u8>,
        algorithm: EncryptionAlgorithm,
    ) {
        self.versions.insert(
            version,
            KeyVersion {
                version,
                material: Zeroizing::new(material),
                algorithm,
            },
        );
        if version > self.current_version {
            self.current_version = version;
        }
    }

    pub(crate) fn current(&self) -> Option<&KeyVersion> {
        self.versions.get(&self.current_version)
    }

    pub(crate) fn version(&self, version: u32) -> Option<&KeyVersion> {
        self.versions.get(&version)
    }
}
