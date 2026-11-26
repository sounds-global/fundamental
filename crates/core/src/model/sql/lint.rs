//! [`Lint`] and [`Level`] — what a migration lint finds, and what a project makes of it.

/// One thing a migration can do that is safe to write and dangerous to run: each with a stable
/// name (`fun.toml` and `#[model(accept(…))]` spell it), a stable code (a finding carries it), a
/// default [`Level`] and the fix a finding says.
///
/// The names and codes never change meaning: a lint that is no longer wanted is kept, at
/// [`Level::Allow`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Lint {
    /// A `DROP`, a `TRUNCATE`, or an `ALTER TABLE` that drops a column or a constraint or renames
    /// a column: it removes data, or a name running code reads.
    Destructive,
    /// A change the engine makes by writing the whole table again: a column's type changed, or
    /// a table's colocation or tablets changed.
    TableRewrite,
    /// The primary key, or how one of its parts is split, changed: a model's key, or `ALTER
    /// TABLE … ADD PRIMARY KEY` on a table that already holds rows.
    KeySplitChanged,
    /// `CREATE INDEX NONCONCURRENTLY` on a table that already holds rows: writes wait for the
    /// whole build.
    BlockingIndex,
    /// One file holds both a schema change and a statement that moves data.
    DdlBesideData,
    /// A key that leads with an ever-increasing value split ascending or descending, such as a
    /// timestamp or a sequence: every new row lands on the same tablet.
    HotShard,
    /// A reference to another record (`<x>_id`) that leads no key and no index: a read by it,
    /// or a delete of the record it names, scans the table.
    UnindexedReference,
    /// A table holding the project's tenant column whose key does not lead with it: one
    /// tenant's rows are spread over every tablet.
    TenantNotLeading,
    /// `NOT NULL` added to an existing table without a default: it fails on the first row.
    NotNullWithoutDefault,
    /// A column's type narrowed: a value that fits today may not fit after.
    NarrowedType,
    /// A migration that writes the rows of a table it did not create: a backfill, run inside
    /// the release.
    Backfill,
    /// A colocated table expected to grow large: an outbox, a table keyed on a timestamp or a
    /// sequence, or one keyed by tenant. Colocated tables share one tablet.
    LargeColocated,
}

/// What a project makes of a [`Lint`] that fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// It blocks, and nothing lets it through.
    Refuse,
    /// It blocks until the table's model accepts it with a reason:
    /// `#[model(accept(<lint> = "reason"))]`.
    Warn,
    /// It is not judged.
    Allow,
}

impl Lint {
    /// Every lint, in code order.
    pub const ALL: [Self; 12] = [
        Self::Destructive,
        Self::TableRewrite,
        Self::KeySplitChanged,
        Self::BlockingIndex,
        Self::DdlBesideData,
        Self::HotShard,
        Self::UnindexedReference,
        Self::TenantNotLeading,
        Self::NotNullWithoutDefault,
        Self::NarrowedType,
        Self::Backfill,
        Self::LargeColocated,
    ];

    /// The name `fun.toml` and `#[model(accept(…))]` spell it with: `hot_shard`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Destructive => "destructive",
            Self::TableRewrite => "table_rewrite",
            Self::KeySplitChanged => "key_split_changed",
            Self::BlockingIndex => "blocking_index",
            Self::DdlBesideData => "ddl_beside_data",
            Self::HotShard => "hot_shard",
            Self::UnindexedReference => "unindexed_reference",
            Self::TenantNotLeading => "tenant_not_leading",
            Self::NotNullWithoutDefault => "not_null_without_default",
            Self::NarrowedType => "narrowed_type",
            Self::Backfill => "backfill",
            Self::LargeColocated => "large_colocated",
        }
    }

    /// The stable code a finding carries: `MIG_L06`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Destructive => "MIG_L01",
            Self::TableRewrite => "MIG_L02",
            Self::KeySplitChanged => "MIG_L03",
            Self::BlockingIndex => "MIG_L04",
            Self::DdlBesideData => "MIG_L05",
            Self::HotShard => "MIG_L06",
            Self::UnindexedReference => "MIG_L07",
            Self::TenantNotLeading => "MIG_L08",
            Self::NotNullWithoutDefault => "MIG_L09",
            Self::NarrowedType => "MIG_L10",
            Self::Backfill => "MIG_L11",
            Self::LargeColocated => "MIG_L12",
        }
    }

    /// The level a project that sets none gets.
    #[must_use]
    pub const fn default_level(self) -> Level {
        match self {
            Self::TableRewrite
            | Self::KeySplitChanged
            | Self::NotNullWithoutDefault
            | Self::NarrowedType => Level::Refuse,
            Self::Destructive
            | Self::BlockingIndex
            | Self::DdlBesideData
            | Self::HotShard
            | Self::UnindexedReference
            | Self::TenantNotLeading
            | Self::Backfill
            | Self::LargeColocated => Level::Warn,
        }
    }

    /// How to fix what it found.
    #[must_use]
    pub const fn fix(self) -> &'static str {
        match self {
            Self::Destructive => {
                "retire it on the model (`#[model(retired(…))]`, or the owner's `retired = \
                 [\"…\"]` for a table), say the column it became `was` it, or name the file in \
                 `drops_by_hand`"
            }
            Self::TableRewrite => {
                "add a new column or table beside it and move to it; colocation and tablets are \
                 right from a table's first migration"
            }
            Self::KeySplitChanged => {
                "create a new table with the new key and move to it; a key is right from the \
                 first migration"
            }
            Self::BlockingIndex => {
                "drop NONCONCURRENTLY: an index on a table with rows is built online"
            }
            Self::DdlBesideData => "move the data statement into a file of its own",
            Self::HotShard => {
                "lead the key with a HASH part, such as the tenant or the id, and order by the \
                 timestamp after it"
            }
            Self::UnindexedReference => {
                "add `#[model(index(name = \"<table>_by_<x>\", columns = [\"<x>_id\"]))]`"
            }
            Self::TenantNotLeading => "lead the key with the tenant column, split HASH",
            Self::NotNullWithoutDefault => {
                "give the column a default, or add it nullable, backfill it, then make it NOT NULL"
            }
            Self::NarrowedType => "add a new column of the narrower type and move to it",
            Self::Backfill => {
                "run the backfill as a job after the release, not inside the migration"
            }
            Self::LargeColocated => {
                "declare the table `#[model(colocated = false)]` so it gets its own tablets"
            }
        }
    }

    /// The lint `name` spells, as [`name`](Self::name) writes it.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|lint| lint.name() == name)
    }
}

impl core::fmt::Display for Lint {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

impl Level {
    /// The word `fun.toml` spells it with: `refuse`, `warn` or `allow`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Refuse => "refuse",
            Self::Warn => "warn",
            Self::Allow => "allow",
        }
    }

    /// The level `word` spells.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        [Self::Refuse, Self::Warn, Self::Allow]
            .into_iter()
            .find(|level| level.name() == word)
    }
}

impl core::fmt::Display for Level {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::{Level, Lint};

    #[test]
    fn every_lint_is_found_by_its_name_and_has_its_own_code() {
        for lint in Lint::ALL {
            assert_eq!(Lint::parse(lint.name()), Some(lint));
            assert_eq!(
                Lint::ALL
                    .iter()
                    .filter(|other| other.code() == lint.code())
                    .count(),
                1,
                "{lint}"
            );
            assert!(!lint.fix().is_empty());
        }
    }

    #[test]
    fn a_name_no_lint_has_is_none() {
        assert_eq!(Lint::parse("hot-shard"), None);
        assert_eq!(Lint::parse(""), None);
    }

    #[test]
    fn what_cannot_be_undone_refuses_and_the_rest_warns_by_default() {
        assert_eq!(Lint::NarrowedType.default_level(), Level::Refuse);
        assert_eq!(Lint::KeySplitChanged.default_level(), Level::Refuse);
        assert_eq!(Lint::HotShard.default_level(), Level::Warn);
        assert_eq!(Lint::Destructive.default_level(), Level::Warn);
    }

    #[test]
    fn a_level_is_read_as_it_is_written() {
        for level in [Level::Refuse, Level::Warn, Level::Allow] {
            assert_eq!(Level::parse(level.name()), Some(level));
        }
        assert_eq!(Level::parse("error"), None);
    }
}
