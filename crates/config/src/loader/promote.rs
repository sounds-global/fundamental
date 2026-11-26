//! Copies the active app's config sections to the top level before typed parsing.

const APP_CONFIG_KEYS: &[&str] = &[
    "database",
    "encryption",
    "vault",
    "messaging",
    "i18n",
    "auth",
    "authz",
    "client",
    "observability",
    "custom",
    "cms",
    "manager",
];

/// Surface entry keys; an unlisted surface would synthesise a `custom` table merged over the root's.
const SURFACE_KEYS: &[&str] = &[
    "surfaces", "api", "web", "ssr", "web-ssr", "hydrate", "sync", "socket", "admin", "native",
];

/// Promotes the active app's config sections to the top level, deep-merged leaf by leaf.
///
/// Any other app subsection becomes a custom section under its own key.
pub(crate) fn promote_app_config(root: &mut toml::Value, app_name: &str) {
    let (mut app_sections, custom_extras) = {
        let toml::Value::Table(root_table) = &*root else {
            return;
        };
        let apps = root_table
            .get("apps")
            .or_else(|| root_table.get("groups"))
            .and_then(|v| v.as_table());
        let Some(apps) = apps else {
            return;
        };
        let Some(app) = apps.get(app_name).and_then(|v| v.as_table()) else {
            return;
        };

        let mut sections = toml::map::Map::new();
        for key in APP_CONFIG_KEYS {
            if let Some(value) = app.get(*key) {
                sections.insert((*key).to_string(), value.clone());
            }
        }

        let mut extras = toml::map::Map::new();
        for (key, value) in app {
            if APP_CONFIG_KEYS.contains(&key.as_str())
                || SURFACE_KEYS.contains(&key.as_str())
                || !value.is_table()
            {
                continue;
            }
            extras.insert(key.clone(), value.clone());
        }
        (sections, extras)
    };

    if !custom_extras.is_empty() {
        let custom = app_sections
            .entry("custom".to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        if let toml::Value::Table(custom_table) = custom {
            for (key, value) in custom_extras {
                custom_table.insert(key, value);
            }
        }
    }

    if app_sections.is_empty() {
        return;
    }

    let toml::Value::Table(root_table) = root else {
        return;
    };
    for (key, value) in app_sections {
        match root_table.get_mut(&key) {
            Some(existing) => super::merge_toml(existing, value),
            None => {
                root_table.insert(key, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn promotes_known_config_sections_to_the_root() {
        let mut root: toml::Value = toml::from_str(
            r#"
            [apps.svc.database.sql]
            url = "postgres://localhost/svc"
            "#,
        )
        .expect("toml");
        promote_app_config(&mut root, "svc");
        assert!(root.get("database").is_some(), "database promoted to root");
    }

    #[test]
    fn promotes_an_unknown_subsection_as_a_custom_section() {
        let mut root: toml::Value = toml::from_str(
            r#"
            [apps.svc]
            api = { port = 16002 }

            [apps.svc.federation]
            entity_id = "https://svc.example"
            ttl = 3600
            "#,
        )
        .expect("toml");
        promote_app_config(&mut root, "svc");

        let custom = root
            .get("custom")
            .and_then(|v| v.as_table())
            .expect("custom");
        let federation = custom
            .get("federation")
            .and_then(|v| v.as_table())
            .expect("federation section");
        assert_eq!(
            federation.get("entity_id").and_then(|v| v.as_str()),
            Some("https://svc.example")
        );
        assert_eq!(
            federation.get("ttl").and_then(|v| v.as_integer()),
            Some(3600)
        );
        assert!(custom.get("api").is_none(), "surface excluded from custom");
    }

    #[test]
    fn merges_custom_sections_with_an_explicit_custom_table() {
        let mut root: toml::Value = toml::from_str(
            r#"
            [apps.svc.custom]
            flag = true

            [apps.svc.federation]
            entity_id = "https://svc.example"
            "#,
        )
        .expect("toml");
        promote_app_config(&mut root, "svc");

        let custom = root
            .get("custom")
            .and_then(|v| v.as_table())
            .expect("custom");
        assert_eq!(custom.get("flag").and_then(|v| v.as_bool()), Some(true));
        assert!(
            custom.get("federation").is_some(),
            "extra custom section kept"
        );
    }

    #[test]
    fn promotes_the_app_scoped_authz_section() {
        let mut root: toml::Value = toml::from_str(
            r#"
            [apps.svc.authz]
            max_eval_depth = 8
            max_reads = 64
            "#,
        )
        .expect("toml");
        promote_app_config(&mut root, "svc");

        let authz = root.get("authz").and_then(|v| v.as_table()).expect("authz");
        assert_eq!(
            authz.get("max_eval_depth").and_then(|v| v.as_integer()),
            Some(8)
        );
        assert_eq!(
            authz.get("max_reads").and_then(|v| v.as_integer()),
            Some(64)
        );
        assert!(
            root.get("custom")
                .and_then(|v| v.as_table())
                .is_none_or(|custom| custom.get("authz").is_none()),
            "authz is a config section, not a custom one"
        );
    }

    #[test]
    fn the_admin_listener_is_a_surface_and_not_a_custom_section() {
        let mut root: toml::Value = toml::from_str(
            r#"
            [apps.svc]
            api = { port = 16002 }
            admin = { host = "127.0.0.1", port = 16102 }
            "#,
        )
        .expect("toml");
        promote_app_config(&mut root, "svc");

        assert!(
            root.get("custom").is_none(),
            "an app that declares only surfaces has no custom sections at all"
        );
    }

    #[test]
    fn a_root_custom_table_survives_an_apps_own_custom_sections() {
        let mut root: toml::Value = toml::from_str(
            r#"
            [custom.origins]
            www = "https://example.test"

            [apps.svc.federation]
            entity_id = "https://svc.example"
            "#,
        )
        .expect("toml");
        promote_app_config(&mut root, "svc");

        let custom = root
            .get("custom")
            .and_then(|v| v.as_table())
            .expect("custom");
        assert!(
            custom.get("origins").is_some(),
            "the root section is still there: {custom:?}"
        );
        assert!(
            custom.get("federation").is_some(),
            "and the app's own section arrived beside it: {custom:?}"
        );
    }

    #[test]
    fn an_app_section_overrides_the_root_leaf_and_keeps_the_rest() {
        let mut root: toml::Value = toml::from_str(
            r#"
            [custom.origins]
            www = "https://root.example"
            business = "https://business.example"

            [apps.svc.custom.origins]
            www = "https://app.example"
            "#,
        )
        .expect("toml");
        promote_app_config(&mut root, "svc");

        let origins = root
            .get("custom")
            .and_then(|v| v.get("origins"))
            .and_then(|v| v.as_table())
            .expect("origins");
        assert_eq!(
            origins.get("www").and_then(|v| v.as_str()),
            Some("https://app.example"),
            "the app's value wins"
        );
        assert_eq!(
            origins.get("business").and_then(|v| v.as_str()),
            Some("https://business.example"),
            "and the key it did not restate is untouched"
        );
    }
}
