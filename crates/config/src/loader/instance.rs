//! App instances: one binary serving a second, separately configured deployment.
//!
//! ```toml
//! [apps.layer0_sandbox]
//! instance_of = "layer0"
//!
//! [apps.layer0_sandbox.database.sql.layer0]
//! url = "postgresql://layer0_sandbox@db/layer0_sandbox"
//! ```
//!
//! Refused: restating a key the base lacks (`CFG_017`), inheriting a secret (`CFG_018`), and
//! leaving out (`CFG_023`) or copying (`CFG_024`) a leaf that names the base's identity.

use fundamental_manifest::APP_LISTENERS;

use crate::ConfigResult;
use crate::error::ConfigError;

use super::merge_toml;

/// The key an instance section declares its base app with.
pub const INSTANCE_OF: &str = "instance_of";

/// The environment variable a deployment names its instance with.
pub const INSTANCE_ENV: &str = "FUNDAMENTAL_INSTANCE";

/// The key an app section names the brand its deployment wears with.
pub const BRAND: &str = "brand";

/// The key an app section lists its own identity-bearing sections and leaves under.
pub const INSTANCE_IDENTITY: &str = "instance_identity";

/// One section of every app that carries a process's identity, and the keys in it that do.
struct IdentityLeaves {
    section: &'static str,
    /// Keys, at any depth, naming who the process is; a table key covers every leaf under it.
    keys: &'static [&'static str],
}

/// Keys not named here (hosts, pools, timeouts, CA paths) are tuning, and inherited.
const IDENTITY_LEAVES: [IdentityLeaves; 4] = [
    IdentityLeaves {
        section: "database",
        keys: &[
            "url",
            "url_template",
            "schema",
            "keyspace",
            "username",
            "password",
            "credentials",
            "tls_client_cert_path",
            "tls_client_cert_pem",
            "tls_client_key_path",
            "tls_client_key_pem",
        ],
    },
    IdentityLeaves {
        section: "messaging",
        keys: &[
            "username",
            "password",
            "token",
            "credentials_path",
            "nkey",
            "tls_client_cert_path",
            "tls_client_key_path",
        ],
    },
    IdentityLeaves {
        section: "vault",
        keys: &[
            "namespace",
            "role",
            "role_id",
            "secret_id",
            "token",
            "jwt_path",
        ],
    },
    IdentityLeaves {
        section: "encryption",
        keys: &["keys", "config_keys", "env_prefix"],
    },
];

/// Resolves which app section this process loads, and writes an instance's effective section.
///
/// # Errors
/// [`ConfigError::AppInstanceUndeclared`], [`ConfigError::AppInstanceOfAnotherApp`],
/// [`ConfigError::AppInstanceRestatesUnknownKey`], [`ConfigError::AppInstanceInheritsSecret`] or
/// [`ConfigError::AppInstanceInheritsIdentity`].
#[expect(clippy::too_many_lines, reason = "readability baseline")]
pub(crate) fn resolve_instance(
    root: &mut toml::Value,
    binary: &str,
    instance: Option<&str>,
) -> ConfigResult<String> {
    let apps = root
        .as_table_mut()
        .and_then(|table| table.get_mut("apps"))
        .and_then(toml::Value::as_table_mut);

    let Some(instance) = instance.filter(|instance| *instance != binary) else {
        if let Some(declared) = apps
            .and_then(|apps| apps.get(binary))
            .and_then(|section| section.get(INSTANCE_OF))
        {
            return Err(ConfigError::AppInstanceOfAnotherApp {
                instance: binary.to_owned(),
                declared: declared.to_string(),
                binary: binary.to_owned(),
            });
        }
        return Ok(binary.to_owned());
    };

    let Some(apps) = apps else {
        return Err(ConfigError::AppInstanceUndeclared {
            instance: instance.to_owned(),
            binary: binary.to_owned(),
        });
    };
    let Some(toml::Value::Table(mut overrides)) = apps.get(instance).cloned() else {
        return Err(ConfigError::AppInstanceUndeclared {
            instance: instance.to_owned(),
            binary: binary.to_owned(),
        });
    };
    match overrides.remove(INSTANCE_OF) {
        Some(toml::Value::String(declared)) if declared == binary => {}
        other => {
            return Err(ConfigError::AppInstanceOfAnotherApp {
                instance: instance.to_owned(),
                declared: other.map_or_else(|| "nothing".to_owned(), |value| value.to_string()),
                binary: binary.to_owned(),
            });
        }
    }
    let mut effective = apps
        .get(binary)
        .cloned()
        .unwrap_or_else(|| toml::Value::Table(toml::map::Map::new()));
    if let Some(key) = first_key_the_base_lacks(&effective, &overrides, "") {
        return Err(ConfigError::AppInstanceRestatesUnknownKey {
            instance: instance.to_owned(),
            key,
            binary: binary.to_owned(),
        });
    }
    if let Some(key) = first_inherited_secret(&effective, &overrides, "") {
        return Err(ConfigError::AppInstanceInheritsSecret {
            instance: instance.to_owned(),
            key,
            binary: binary.to_owned(),
        });
    }
    if let Some(key) = first_inherited_identity(&effective, &overrides) {
        return Err(ConfigError::AppInstanceInheritsIdentity {
            instance: instance.to_owned(),
            key,
            binary: binary.to_owned(),
        });
    }
    if let Some(key) = first_copied_identity(&effective, &overrides) {
        return Err(ConfigError::AppInstanceSharesIdentity {
            instance: instance.to_owned(),
            key,
            binary: binary.to_owned(),
        });
    }
    merge_toml(&mut effective, toml::Value::Table(overrides));
    apps.insert(instance.to_owned(), effective);
    Ok(instance.to_owned())
}

/// Takes each listener's `host`, `tls` and `listen` out of an instance's runtime variables.
pub(crate) fn take_listener_addresses(section: &mut toml::Value) -> Option<toml::Value> {
    let section = section.as_table_mut()?;
    let mut taken = toml::map::Map::new();
    for listener in APP_LISTENERS {
        let Some(table) = section
            .get_mut(listener)
            .and_then(toml::Value::as_table_mut)
        else {
            continue;
        };
        let mut address = toml::map::Map::new();
        for key in ["host", "tls", "listen"] {
            if let Some(value) = table.remove(key) {
                address.insert(key.to_owned(), value);
            }
        }
        if table.is_empty() {
            section.remove(listener);
        }
        if !address.is_empty() {
            taken.insert(listener.to_owned(), toml::Value::Table(address));
        }
    }
    (!taken.is_empty()).then_some(toml::Value::Table(taken))
}

fn first_key_the_base_lacks(
    base: &toml::Value,
    overrides: &toml::map::Map<String, toml::Value>,
    prefix: &str,
) -> Option<String> {
    overrides.iter().find_map(|(key, value)| {
        let path = join(prefix, key);
        match (base.get(key), value) {
            (None, _) => Some(path),
            (Some(base_value @ toml::Value::Table(_)), toml::Value::Table(nested)) => {
                first_key_the_base_lacks(base_value, nested, &path)
            }
            _ => None,
        }
    })
}

/// A value whose every string holds a `${…}` reference is allowed: each deployment resolves it.
fn first_copied_identity(
    base: &toml::Value,
    overrides: &toml::map::Map<String, toml::Value>,
) -> Option<String> {
    let mut units = Vec::new();
    for leaves in &IDENTITY_LEAVES {
        if let Some(section) = base.get(leaves.section) {
            identity_keys(
                section,
                leaves.keys,
                vec![leaves.section.to_owned()],
                &mut units,
            );
        }
    }
    for path in listed_paths(base) {
        let path: Vec<String> = path.into_iter().map(str::to_owned).collect();
        match path.iter().try_fold(base, |value, key| value.get(key)) {
            Some(toml::Value::Table(section)) => units.extend(section.keys().map(|key| {
                let mut unit = path.clone();
                unit.push(key.clone());
                unit
            })),
            Some(_) => units.push(path),
            None => {}
        }
    }
    units.into_iter().find_map(|unit| {
        let original = unit.iter().try_fold(base, |value, key| value.get(key))?;
        let (first, rest) = unit.split_first()?;
        let restated = rest
            .iter()
            .try_fold(overrides.get(first)?, |value, key| value.get(key))?;
        (restated == original && !resolved_per_deployment(original)).then(|| unit.join("."))
    })
}

fn listed_paths(base: &toml::Value) -> Vec<Vec<&str>> {
    base.get(INSTANCE_IDENTITY)
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
        .flat_map(|entry| {
            let path: Vec<&str> = entry.split('.').collect();
            let under_custom: Vec<&str> = ["custom"].into_iter().chain(path.clone()).collect();
            [path, under_custom]
        })
        .collect()
}

fn identity_keys(
    value: &toml::Value,
    keys: &[&str],
    path: Vec<String>,
    units: &mut Vec<Vec<String>>,
) {
    let toml::Value::Table(table) = value else {
        return;
    };
    for (key, nested) in table {
        let mut unit = path.clone();
        unit.push(key.clone());
        if LOCATION_KEYS.contains(&key.as_str()) {
            continue;
        }
        if keys.contains(&key.as_str()) {
            units.push(unit);
        } else {
            identity_keys(nested, keys, unit, units);
        }
    }
}

/// Identity keys naming a place inside another identity, where equal text names different objects.
const LOCATION_KEYS: [&str; 5] = [
    "schema",
    "tls_client_cert_path",
    "tls_client_key_path",
    "credentials_path",
    "jwt_path",
];

/// Every string holds a `${…}` reference; one literal beside a reference is shared text.
fn resolved_per_deployment(value: &toml::Value) -> bool {
    let mut strings = Vec::new();
    collect_strings(value, &mut strings);
    !strings.is_empty() && strings.iter().all(|text| text.contains("${"))
}

fn collect_strings<'a>(value: &'a toml::Value, strings: &mut Vec<&'a str>) {
    match value {
        toml::Value::String(text) => strings.push(text),
        toml::Value::Array(items) => items.iter().for_each(|item| collect_strings(item, strings)),
        toml::Value::Table(table) => table
            .values()
            .for_each(|item| collect_strings(item, strings)),
        _ => {}
    }
}

/// The first identity leaf `overrides` leaves out, or `identity` itself when it redeclares the list.
fn first_inherited_identity(
    base: &toml::Value,
    overrides: &toml::map::Map<String, toml::Value>,
) -> Option<String> {
    if overrides.contains_key(INSTANCE_IDENTITY) {
        return Some(INSTANCE_IDENTITY.to_owned());
    }
    let declared = IDENTITY_LEAVES.iter().find_map(|leaves| {
        let section = base.get(leaves.section).filter(|value| value.is_table())?;
        first_identity_left_out(
            section,
            overrides.get(leaves.section),
            leaves.keys,
            leaves.section,
        )
    });
    declared.or_else(|| {
        listed_paths(base).into_iter().find_map(|path| {
            let value = path.iter().try_fold(base, |value, key| value.get(key))?;
            let restated = path.split_first().and_then(|(first, rest)| {
                rest.iter()
                    .try_fold(overrides.get(*first)?, |value, key| value.get(key))
            });
            first_leaf_left_out(value, restated, &path.join("."))
        })
    })
}

fn first_identity_left_out(
    base: &toml::Value,
    restated: Option<&toml::Value>,
    keys: &[&str],
    prefix: &str,
) -> Option<String> {
    let toml::Value::Table(table) = base else {
        return None;
    };
    table.iter().find_map(|(key, value)| {
        let path = join(prefix, key);
        let restated = restated.and_then(|restated| restated.get(key));
        if keys.contains(&key.as_str()) {
            first_leaf_left_out(value, restated, &path)
        } else {
            first_identity_left_out(value, restated, keys, &path)
        }
    })
}

fn first_leaf_left_out(
    base: &toml::Value,
    restated: Option<&toml::Value>,
    prefix: &str,
) -> Option<String> {
    let toml::Value::Table(table) = base else {
        return restated.is_none().then(|| prefix.to_owned());
    };
    if table.is_empty() {
        return restated.is_none().then(|| prefix.to_owned());
    }
    table.iter().find_map(|(key, value)| {
        first_leaf_left_out(
            value,
            restated.and_then(|restated| restated.get(key)),
            &join(prefix, key),
        )
    })
}

fn first_inherited_secret(
    base: &toml::Value,
    overrides: &toml::map::Map<String, toml::Value>,
    prefix: &str,
) -> Option<String> {
    let toml::Value::Table(table) = base else {
        return None;
    };
    table.iter().find_map(|(key, value)| {
        let path = join(prefix, key);
        match (value, overrides.get(key)) {
            (toml::Value::Table(_), Some(toml::Value::Table(nested))) => {
                first_inherited_secret(value, nested, &path)
            }
            (toml::Value::Table(_), None) => {
                first_inherited_secret(value, &toml::map::Map::new(), &path)
            }
            _ if !bears_a_secret(key, value) => None,
            (_, Some(restated)) if restated != value => None,
            _ => Some(path),
        }
    })
}

fn bears_a_secret(key: &str, value: &toml::Value) -> bool {
    key.ends_with("key_file") || key.ends_with("key_path") || names_a_secret_file(value)
}

fn names_a_secret_file(value: &toml::Value) -> bool {
    match value {
        toml::Value::String(text) => text.contains("${file:"),
        toml::Value::Array(items) => items.iter().any(names_a_secret_file),
        toml::Value::Table(table) => table.iter().any(|(key, value)| bears_a_secret(key, value)),
        _ => false,
    }
}

fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_owned()
    } else {
        format!("{prefix}.{key}")
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_instance;
    use crate::error::ConfigError;

    fn manifest(text: &str) -> toml::Value {
        toml::from_str(text).expect("a manifest")
    }

    const LAYER0: &str = r#"
        [apps.layer0.database.sql.layer0]
        url = "postgresql://layer0@db/layer0"
        max_connections = 8

        [apps.layer0.admission]
        reads = 192
        writes = 96

        [apps.layer0_sandbox]
        instance_of = "layer0"

        [apps.layer0_sandbox.database.sql.layer0]
        url = "postgresql://layer0_sandbox@db/layer0_sandbox"
        max_connections = 4

        [apps.layer0_sandbox.admission]
        writes = 0
    "#;

    #[test]
    fn an_instance_merges_its_sections_over_its_base_leaf_by_leaf() {
        let mut root = manifest(LAYER0);
        let name = resolve_instance(&mut root, "layer0", Some("layer0_sandbox")).expect("resolves");
        assert_eq!(name, "layer0_sandbox");
        let section = &root["apps"]["layer0_sandbox"];
        assert_eq!(
            section["database"]["sql"]["layer0"]["url"].as_str(),
            Some("postgresql://layer0_sandbox@db/layer0_sandbox"),
            "the instance's own value"
        );
        assert_eq!(section["admission"]["writes"].as_integer(), Some(0));
        assert_eq!(
            section["admission"]["reads"].as_integer(),
            Some(192),
            "a key the instance does not restate is the base's"
        );
        assert!(
            section.get("instance_of").is_none(),
            "the declaration is not promoted as a custom section"
        );
        assert_eq!(
            root["apps"]["layer0"]["database"]["sql"]["layer0"]["url"].as_str(),
            Some("postgresql://layer0@db/layer0"),
            "the base is untouched"
        );
    }

    #[test]
    fn no_instance_loads_the_binarys_own_section() {
        let mut root = manifest(LAYER0);
        assert_eq!(
            resolve_instance(&mut root, "layer0", None).expect("resolves"),
            "layer0"
        );
        assert_eq!(
            resolve_instance(&mut root, "layer0", Some("layer0")).expect("resolves"),
            "layer0"
        );
    }

    #[test]
    fn an_undeclared_instance_refuses() {
        let mut root = manifest(LAYER0);
        let error = resolve_instance(&mut root, "layer0", Some("layer0_staging")).unwrap_err();
        assert!(matches!(error, ConfigError::AppInstanceUndeclared { .. }));
        assert_eq!(fundamental_core::Error::error_code(&error), "CFG_015");
    }

    #[test]
    fn a_binary_cannot_boot_as_another_apps_instance() {
        let mut root = manifest(LAYER0);
        let error = resolve_instance(&mut root, "layer2", Some("layer0_sandbox")).unwrap_err();
        assert!(matches!(error, ConfigError::AppInstanceOfAnotherApp { .. }));
        assert_eq!(fundamental_core::Error::error_code(&error), "CFG_016");
    }

    #[test]
    fn a_section_that_declares_no_base_is_not_an_instance() {
        let mut root = manifest(
            r#"
            [apps.layer0.api]
            port = 1
            [apps.stray]
            api = { port = 2 }
        "#,
        );
        let error = resolve_instance(&mut root, "layer0", Some("stray")).unwrap_err();
        assert!(matches!(error, ConfigError::AppInstanceOfAnotherApp { .. }));
    }

    #[test]
    fn an_instance_restating_a_key_its_base_lacks_refuses() {
        let mut root = manifest(
            r#"
            [apps.layer0.admission]
            writes = 96
            [apps.layer0_sandbox]
            instance_of = "layer0"
            [apps.layer0_sandbox.admission]
            wirtes = 0
        "#,
        );
        let error = resolve_instance(&mut root, "layer0", Some("layer0_sandbox")).unwrap_err();
        assert!(
            matches!(&error, ConfigError::AppInstanceRestatesUnknownKey { key, .. } if key == "admission.wirtes"),
            "{error}"
        );
        assert_eq!(fundamental_core::Error::error_code(&error), "CFG_017");
    }

    const SECRETS: &str = r#"
        [apps.layer0.federation]
        signing_key_pem = "${file:secrets/layer0_genesis_key.pem:-}"
        kid = "genesis-1"
        [apps.layer0.auth]
        token_public_key_file = "secrets/layer0_auth_public.pem"
        [apps.layer0.encryption]
        config_keys = [{ id = "outbox_seal", material = "${file:secrets/layer0_outbox_seal.v1.key:-}" }]
    "#;

    fn with_instance(overrides: &str) -> toml::Value {
        manifest(&format!(
            "{SECRETS}\n[apps.layer0_sandbox]\ninstance_of = \"layer0\"\n{overrides}"
        ))
    }

    const OWN_AUTH: &str = r#"
        [apps.layer0_sandbox.auth]
        token_public_key_file = "secrets/sandbox/layer0_auth_public.pem"
    "#;

    const OWN_SEAL: &str = r#"
        [apps.layer0_sandbox.encryption]
        config_keys = [{ id = "outbox_seal", material = "${file:secrets/sandbox/layer0_outbox_seal.v1.key:-}" }]
    "#;

    const RESTATED_GENESIS: &str = r#"
        [apps.layer0_sandbox.federation]
        signing_key_pem = "${file:secrets/layer0_genesis_key.pem:-}"
    "#;

    const OWN_GENESIS: &str = r#"
        [apps.layer0_sandbox.federation]
        signing_key_pem = "${file:secrets/sandbox/layer0_genesis_key.pem:-}"
    "#;

    #[test]
    fn an_instance_that_inherits_a_secret_reference_refuses() {
        for (overrides, inherited) in [
            (String::new(), "auth.token_public_key_file"),
            (OWN_AUTH.to_owned(), "encryption.config_keys"),
            (
                format!("{OWN_AUTH}{OWN_SEAL}{RESTATED_GENESIS}"),
                "federation.signing_key_pem",
            ),
        ] {
            let mut root = with_instance(&overrides);
            let error = resolve_instance(&mut root, "layer0", Some("layer0_sandbox")).unwrap_err();
            assert!(
                matches!(&error, ConfigError::AppInstanceInheritsSecret { key, .. } if key == inherited),
                "expected {inherited} to be refused, got {error}"
            );
            assert_eq!(fundamental_core::Error::error_code(&error), "CFG_018");
        }
    }

    #[test]
    fn an_instance_that_restates_every_secret_loads() {
        let mut root = with_instance(&format!("{OWN_AUTH}{OWN_SEAL}{OWN_GENESIS}"));
        resolve_instance(&mut root, "layer0", Some("layer0_sandbox"))
            .expect("every secret is its own");
        assert_eq!(
            root["apps"]["layer0_sandbox"]["federation"]["kid"].as_str(),
            Some("genesis-1"),
            "a leaf naming no secret is still inherited"
        );
    }

    #[test]
    fn a_binarys_own_section_may_not_claim_to_be_an_instance() {
        let mut root = manifest(
            r#"
            [apps.layer0]
            instance_of = "layer2"
        "#,
        );
        let error = resolve_instance(&mut root, "layer0", None).unwrap_err();
        assert!(matches!(error, ConfigError::AppInstanceOfAnotherApp { .. }));
    }

    const IDENTITY: &str = r#"
        [apps.www]
        brand = "dagen"
        instance_identity = ["mail.from_address"]

        [apps.www.database.sql.www]
        url = "postgresql://www_dagen@db/www_dagen"
        max_connections = 8
        tls_ca_cert_path = "/certs/ca.crt"

        [apps.www.database.cql.www]
        hosts = ["cql:9042"]
        keyspace = "www_dagen"
        username = "www_dagen"
        password = "${CQL_WWW_DAGEN_PASSWORD}"
        connections_per_host = 2

        [apps.www.messaging.nats]
        url = "nats://nats:4222"
        username = "www_dagen"
        password = "${NATS_WWW_DAGEN_PASSWORD}"
        inbox_prefix = "_INBOX.www"

        [apps.www.vault]
        address = "https://bao:8200"
        namespace = "dagen"
        auth = { method = "app_role", role_id = "www_dagen", secret_id = "${file:secrets/bao_www_dagen:-}" }

        [apps.www.encryption]
        backend = "transit"
        keys = [{ id = "pii", encrypted_dek = "vault:v1:x", transit_key = "www_dagen" }]

        [apps.www.mail]
        from_address = "hallo@dagen.nl"
        reply_window_days = 14

        [apps.www.admission]
        reads = 192

        [apps.www_days]
        instance_of = "www"
        brand = "days"
    "#;

    const DAYS: &str = r#"
        [apps.www_days.database.sql.www]
        url = "postgresql://www_days@db/www_days"

        [apps.www_days.database.cql.www]
        keyspace = "www_days"
        username = "www_days"
        password = "${CQL_WWW_DAYS_PASSWORD}"

        [apps.www_days.messaging.nats]
        username = "www_days"
        password = "${NATS_WWW_DAYS_PASSWORD}"

        [apps.www_days.vault]
        namespace = "days"
        auth = { role_id = "www_days", secret_id = "${file:secrets/bao_www_days:-}" }

        [apps.www_days.encryption]
        keys = [{ id = "pii", encrypted_dek = "vault:v1:y", transit_key = "www_days" }]

        [apps.www_days.mail]
        from_address = "hello@days.com"
    "#;

    fn days_without(left_out: &[&str]) -> toml::Value {
        let mut root = manifest(&format!("{IDENTITY}{DAYS}"));
        let (leaf, parents) = left_out.split_last().expect("a leaf to leave out");
        let parent = parents
            .iter()
            .try_fold(&mut root["apps"]["www_days"], |value, key| {
                value.get_mut(*key)
            })
            .and_then(toml::Value::as_table_mut)
            .expect("the leaf's table");
        assert!(parent.remove(*leaf).is_some(), "{left_out:?} was restated");
        root
    }

    fn inherited_identity(root: &mut toml::Value) -> Option<String> {
        match resolve_instance(root, "www", Some("www_days")) {
            Err(error @ ConfigError::AppInstanceInheritsIdentity { .. }) => {
                assert_eq!(fundamental_core::Error::error_code(&error), "CFG_023");
                match error {
                    ConfigError::AppInstanceInheritsIdentity { key, .. } => Some(key),
                    _ => None,
                }
            }
            Err(other) => panic!("expected CFG_023, got {other}"),
            Ok(_) => None,
        }
    }

    #[test]
    fn an_instance_inheriting_any_identity_leaf_is_refused() {
        for left_out in [
            &["database", "sql", "www", "url"][..],
            &["database", "cql", "www", "keyspace"],
            &["database", "cql", "www", "username"],
            &["database", "cql", "www", "password"],
            &["messaging", "nats", "username"],
            &["messaging", "nats", "password"],
            &["vault", "namespace"],
            &["vault", "auth", "role_id"],
            &["encryption", "keys"],
            &["mail", "from_address"],
        ] {
            assert_eq!(
                inherited_identity(&mut days_without(left_out)),
                Some(left_out.join(".")),
                "leaving out {left_out:?} must be refused"
            );
        }
    }

    fn days_copying(copied: &[&str]) -> toml::Value {
        let mut root = manifest(&format!("{IDENTITY}{DAYS}"));
        let original = copied
            .iter()
            .try_fold(&root["apps"]["www"], |value, key| value.get(*key))
            .expect("the base's value")
            .clone();
        let (leaf, parents) = copied.split_last().expect("a value to copy");
        parents
            .iter()
            .try_fold(&mut root["apps"]["www_days"], |value, key| {
                value.get_mut(*key)
            })
            .and_then(toml::Value::as_table_mut)
            .expect("the value's table")
            .insert((*leaf).to_owned(), original);
        root
    }

    fn shared_identity(root: &mut toml::Value) -> Option<String> {
        match resolve_instance(root, "www", Some("www_days")) {
            Err(ConfigError::AppInstanceSharesIdentity { key, .. }) => Some(key),
            Err(other) => panic!("expected CFG_024, got {other}"),
            Ok(_) => None,
        }
    }

    #[test]
    fn an_instance_copying_any_identity_value_is_refused() {
        for copied in [
            &["database", "sql", "www", "url"][..],
            &["database", "cql", "www", "keyspace"],
            &["database", "cql", "www", "username"],
            &["messaging", "nats", "username"],
            &["vault", "namespace"],
            &["vault", "auth", "role_id"],
            &["encryption", "keys"],
            &["mail", "from_address"],
        ] {
            assert_eq!(
                shared_identity(&mut days_copying(copied)),
                Some(copied.join(".")),
                "copying {copied:?} must be refused"
            );
        }
        let error = resolve_instance(
            &mut days_copying(&["vault", "namespace"]),
            "www",
            Some("www_days"),
        )
        .unwrap_err();
        assert_eq!(fundamental_core::Error::error_code(&error), "CFG_024");
    }

    #[test]
    fn an_instance_copying_a_key_of_a_section_listed_whole_is_refused() {
        let base = IDENTITY.replace(
            r#"instance_identity = ["mail.from_address"]"#,
            r#"instance_identity = ["mail.from_address", "steward"]
        steward = { role_id = "steward_dagen", password = { source = "vault_kv", path = "steward/dagen" } }"#,
        );
        let own = r#"
        steward = { role_id = "steward_days", password = { source = "vault_kv", path = "steward/days" } }"#;
        let copied = r#"
        steward = { role_id = "steward_dagen", password = { source = "vault_kv", path = "steward/days" } }"#;
        let with = |steward: &str| {
            manifest(
                &format!("{base}{DAYS}")
                    .replace(r#"brand = "days""#, &format!("brand = \"days\"\n{steward}")),
            )
        };
        resolve_instance(&mut with(own), "www", Some("www_days"))
            .expect("a section whose every key is its own, a nested source alike");
        assert_eq!(
            shared_identity(&mut with(copied)),
            Some("steward.role_id".to_owned())
        );
    }

    #[test]
    fn a_copied_section_written_under_custom_is_refused() {
        let base = IDENTITY.replace(
            r#"instance_identity = ["mail.from_address"]"#,
            r#"instance_identity = ["mail.from_address", "steward"]
        custom = { steward = { role_id = "steward_dagen" } }"#,
        );
        let mut root = manifest(&format!("{base}{DAYS}").replace(
            r#"brand = "days""#,
            r#"brand = "days"
        custom = { steward = { role_id = "steward_dagen" } }"#,
        ));
        assert_eq!(
            shared_identity(&mut root),
            Some("custom.steward.role_id".to_owned())
        );
    }

    #[test]
    fn a_literal_beside_a_reference_is_still_a_copy() {
        let mut root = manifest(&format!("{IDENTITY}{DAYS}"));
        let credentials: toml::Value =
            toml::from_str(r#"credentials = { role = "www_dagen", password = "${CRED}" }"#)
                .expect("a table");
        for app in ["www", "www_days"] {
            root["apps"][app]["database"]["cql"]["www"]
                .as_table_mut()
                .expect("the cql section")
                .insert("credentials".to_owned(), credentials["credentials"].clone());
        }
        assert_eq!(
            shared_identity(&mut root),
            Some("database.cql.www.credentials".to_owned())
        );
    }

    #[test]
    fn an_equal_location_inside_its_own_identity_is_not_a_copy() {
        let mut root = manifest(&format!("{IDENTITY}{DAYS}"));
        for app in ["www", "www_days"] {
            root["apps"][app]["database"]["sql"]["www"]
                .as_table_mut()
                .expect("the sql section")
                .insert(
                    "schema".to_owned(),
                    toml::Value::String("public".to_owned()),
                );
        }
        resolve_instance(&mut root, "www", Some("www_days"))
            .expect("one schema name in two databases names two schemas");
    }

    #[test]
    fn an_equal_reference_is_per_deployment_indirection() {
        let mut root = days_copying(&["database", "cql", "www", "password"]);
        resolve_instance(&mut root, "www", Some("www_days"))
            .expect("a `${…}` reference resolves in each deployment's own environment");
    }

    #[test]
    fn an_instance_restating_its_identity_inherits_its_tuning() {
        let mut root = manifest(&format!("{IDENTITY}{DAYS}"));
        resolve_instance(&mut root, "www", Some("www_days")).expect("its identity is its own");
        let section = &root["apps"]["www_days"];
        assert_eq!(section["brand"].as_str(), Some("days"));
        assert_eq!(
            section["vault"]["auth"]["role_id"].as_str(),
            Some("www_days")
        );
        for (path, inherited) in [
            ("database.sql.www.max_connections", toml::Value::Integer(8)),
            ("database.sql.www.tls_ca_cert_path", "/certs/ca.crt".into()),
            (
                "database.cql.www.hosts",
                toml::Value::Array(vec!["cql:9042".into()]),
            ),
            ("messaging.nats.url", "nats://nats:4222".into()),
            ("messaging.nats.inbox_prefix", "_INBOX.www".into()),
            ("vault.address", "https://bao:8200".into()),
            ("vault.auth.method", "app_role".into()),
            ("encryption.backend", "transit".into()),
            ("mail.reply_window_days", toml::Value::Integer(14)),
            ("admission.reads", toml::Value::Integer(192)),
        ] {
            let value = path
                .split('.')
                .try_fold(section, |value, key| value.get(key));
            assert_eq!(value, Some(&inherited), "{path} is the base's tuning");
        }
    }

    #[test]
    fn every_declared_identity_key_is_a_setting_of_its_section() {
        let settings = [
            (
                "database",
                concat!(
                    include_str!("../settings/database/sql.rs"),
                    include_str!("../settings/database/cql.rs")
                ),
            ),
            ("messaging", include_str!("../settings/message/nats.rs")),
            ("vault", include_str!("../settings/vault.rs")),
            ("encryption", include_str!("../settings/encryption.rs")),
        ];
        for leaves in &super::IDENTITY_LEAVES {
            let (_, source) = settings
                .iter()
                .find(|(section, _)| *section == leaves.section)
                .expect("every identity section has settings");
            for key in leaves.keys {
                assert!(
                    source.contains(&format!(" {key}: ")),
                    "`{}.{key}` names no field of its settings",
                    leaves.section
                );
            }
        }
    }

    #[test]
    fn an_identity_custom_section_may_live_under_custom() {
        let base = r#"
            [apps.www]
            instance_identity = ["passkey"]
            [apps.www.custom.passkey]
            rp_id = "mijn.dagen.nl"
            [apps.www_days]
            instance_of = "www"
        "#;
        let refused = inherited_identity(&mut manifest(base));
        assert_eq!(refused.as_deref(), Some("custom.passkey.rp_id"));
    }

    #[test]
    fn an_instance_may_not_redeclare_its_identity_sections() {
        let mut root = manifest(&format!("{IDENTITY}{DAYS}"));
        let instance = root["apps"]["www_days"]
            .as_table_mut()
            .expect("the instance section");
        instance.insert(
            "instance_identity".to_owned(),
            toml::Value::Array(Vec::new()),
        );
        assert_eq!(
            inherited_identity(&mut root).as_deref(),
            Some("instance_identity")
        );
    }
}
