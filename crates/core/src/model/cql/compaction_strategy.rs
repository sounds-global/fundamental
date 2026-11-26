//! Compaction strategies selectable for a YCQL table.

use super::TimeWindowUnit;

/// Compaction strategy for YCQL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactionStrategy {
    /// Size-tiered compaction (good for write-heavy workloads).
    SizeTiered,
    /// Leveled compaction (good for read-heavy workloads).
    Leveled,
    /// Time-window compaction (good for time-series data).
    TimeWindow {
        /// Window size unit.
        unit: TimeWindowUnit,
        /// Window size.
        size: u32,
    },
}
