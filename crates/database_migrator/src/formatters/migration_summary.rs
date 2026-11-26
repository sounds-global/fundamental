//! Renders a single [`Migration`] as a human-readable summary.

use fundamental_db::migration::Migration;

/// Formats a migration for display.
pub fn format_migration_summary(migration: &Migration) -> String {
    let mut output = String::new();

    output.push_str(&format!(
        "{} {} ({})\n",
        migration.safety_tier.emoji(),
        migration.name,
        migration.safety_tier
    ));

    for change in &migration.changes {
        output.push_str(&format!(
            "  {} {}\n",
            change.safety_tier().emoji(),
            change.description()
        ));
    }

    output
}
