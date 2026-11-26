/// Where a signing key's private half is held, as distinct from how it is protected.
///
/// Tells a software key in this process from one behind non-exportable custody.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyResidence {
    /// In this process's memory, where code running in the process can copy it.
    InProcess,
    /// Behind a custody boundary that reports the key non-exportable; the process holds only a
    /// handle.
    NonExportableCustody,
}

impl KeyResidence {
    /// The stable token for logs and descriptors (`in-process` / `non-exportable-custody`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InProcess => "in-process",
            Self::NonExportableCustody => "non-exportable-custody",
        }
    }
}
