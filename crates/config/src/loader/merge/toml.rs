//! Recursive deep merge of TOML values used when layering config files.

/// Deep merges two TOML values, tables key by key; any other value replaces `base` wholesale.
pub(crate) fn merge_toml(base: &mut toml::Value, override_val: toml::Value) {
    match (base, override_val) {
        (toml::Value::Table(base_table), toml::Value::Table(override_table)) => {
            for (key, value) in override_table {
                match base_table.get_mut(&key) {
                    Some(base_value) => merge_toml(base_value, value),
                    None => {
                        base_table.insert(key, value);
                    }
                }
            }
        }
        (base, override_val) => {
            *base = override_val;
        }
    }
}
