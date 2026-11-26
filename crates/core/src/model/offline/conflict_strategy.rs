/// How an offline record changed both locally and on the server is resolved; set with `OfflineModel::CONFLICT_RESOLUTION`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ConflictStrategy {
    /// The record with the latest `updated_at` wins.
    #[default]
    LastWriteWins,

    /// Server changes overwrite local ones.
    ServerWins,

    /// Local changes overwrite the server's.
    ClientWins,

    /// The application decides: `resolve_conflict` returns `Resolution::Manual`.
    Custom,

    /// Non-conflicting field changes merge; conflicting fields fall back to `LastWriteWins`.
    Merge,
}

impl ConflictStrategy {
    /// Returns the string representation.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::LastWriteWins => "last_write_wins",
            Self::ServerWins => "server_wins",
            Self::ClientWins => "client_wins",
            Self::Custom => "custom",
            Self::Merge => "merge",
        }
    }

    /// Parses from a string.
    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "last_write_wins" => Some(Self::LastWriteWins),
            "server_wins" => Some(Self::ServerWins),
            "client_wins" => Some(Self::ClientWins),
            "custom" => Some(Self::Custom),
            "merge" => Some(Self::Merge),
            _ => None,
        }
    }
}
