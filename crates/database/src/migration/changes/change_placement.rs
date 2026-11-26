//! The [`ChangePlacementChange`] schema change: a table placed otherwise than its model says.

/// A table whose colocation differs from its model's, or that is split into fewer tablets than
/// its model declares. More tablets are no drift: the engine splits a growing table on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangePlacementChange {
    /// Table name.
    pub table_name: String,
    /// Whether the table is colocated, and whether its model says it is, when either differs.
    pub colocated: Option<(bool, bool)>,
    /// The tablets the table has, and the tablets its model declares, when it has fewer.
    pub tablets: Option<(Option<u32>, u32)>,
}

impl ChangePlacementChange {
    /// What the change does, in words.
    #[must_use]
    pub fn description(&self) -> String {
        let mut said = Vec::new();
        if let Some((is, declared)) = self.colocated {
            let word = |colocated: bool| if colocated { "colocated" } else { "apart" };
            said.push(format!(
                "it is {}, its model says {}",
                word(is),
                word(declared)
            ));
        }
        if let Some((has, declared)) = self.tablets {
            let has = has.map_or_else(|| "no count of".to_owned(), |n| n.to_string());
            said.push(format!(
                "it has {has} tablets, its model declares {declared}"
            ));
        }
        format!(
            "Place '{}' as its model says ({}; recreate the table to change it)",
            self.table_name,
            said.join("; ")
        )
    }
}
