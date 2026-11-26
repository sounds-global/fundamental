//! Configuration loader ([`ConfigLoader`]).

mod config;
mod instance;
mod merge;
pub mod parse;
mod promote;

pub use config::ConfigLoader;
pub use instance::{BRAND, INSTANCE_ENV, INSTANCE_IDENTITY, INSTANCE_OF};
pub(crate) use instance::{resolve_instance, take_listener_addresses};
pub(crate) use promote::promote_app_config;

pub(crate) use merge::*;
pub(crate) use parse::*;

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Environment;
    use fundamental_core::Error as _;
    use tempfile::TempDir;

    #[test]
    fn a_relative_secret_reference_resolves_against_the_config_not_the_cwd() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");
        std::fs::create_dir_all(dir.path().join("secrets")).unwrap();
        std::fs::write(
            dir.path().join("secrets").join("api_url"),
            "https://from-file\n",
        )
        .unwrap();

        std::fs::write(
            &config_path,
            r#"
            [project]
            name = "test-app"

            [apps.main]
            web = { host = "0.0.0.0", port = 8080 }

            [apps.main.client]
            api_url = "${file:secrets/api_url}"
            "#,
        )
        .unwrap();

        let config = ConfigLoader::development()
            .with_fundamental_toml(&config_path)
            .load()
            .expect("the reference resolves against the config's own directory");

        assert_eq!(
            config.client.api_url, "https://from-file",
            "the trailing newline every tool leaves on a secret file is stripped"
        );
    }

    #[test]
    fn a_relative_secret_resolves_from_the_root() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("secrets")).unwrap();
        std::fs::write(
            dir.path().join("secrets/api_url"),
            "https://from-the-root\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("fun.toml"),
            "[project]\nname = \"test-app\"\n\n[apps.main]\nweb = { host = \"0.0.0.0\", port = 8080 }\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("fun.local.toml"),
            "[apps.main.client]\napi_url = \"${file:secrets/api_url}\"\n",
        )
        .unwrap();

        let config = ConfigLoader::development()
            .with_fundamental_toml(dir.path().join("fun.toml"))
            .load()
            .expect("the settings load");

        assert_eq!(config.client.api_url, "https://from-the-root");
    }

    #[test]
    fn test_loader_loads_fundamental_toml() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [project]
            name = "test-app"

            [apps.main]
            web = { host = "0.0.0.0", port = 8080 }

            [apps.main.client]
            api_url = "https://api.example.com"
            "#,
        )
        .unwrap();

        let config = ConfigLoader::development()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();

        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.client.api_url, "https://api.example.com");
    }

    #[test]
    fn test_loader_applies_environment_overrides() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [apps.main]
            web = { host = "127.0.0.1", port = 3000 }
            "#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("fun.production.toml"),
            "[apps.main]\nweb = { host = \"0.0.0.0\", port = 8080 }\n",
        )
        .unwrap();

        let dev_config = ConfigLoader::development()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();
        assert_eq!(dev_config.server.host, "127.0.0.1");
        assert_eq!(dev_config.server.port, 3000);

        let prod_config = ConfigLoader::production()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();
        assert_eq!(prod_config.server.host, "0.0.0.0");
        assert_eq!(prod_config.server.port, 8080);
    }

    #[test]
    fn test_loader_merges_sources() {
        let dir = TempDir::new().unwrap();

        let fundamental_path = dir.path().join("fun.toml");
        std::fs::write(
            &fundamental_path,
            r#"
            [apps.main]
            web = { host = "127.0.0.1", port = 3000 }
            "#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join("fun.local.toml"),
            r#"
            [apps.main]
            web = { port = 3001 }
            "#,
        )
        .unwrap();

        let config = ConfigLoader::development()
            .with_fundamental_toml(&fundamental_path)
            .load()
            .unwrap();

        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 3001);
    }

    #[test]
    fn test_loader_expands_env_vars_with_defaults() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [apps.main.client]
            api_url = "${NONEXISTENT_VAR:-https://default.example.com}"
            "#,
        )
        .unwrap();

        let config = ConfigLoader::development()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();

        assert_eq!(config.client.api_url, "https://default.example.com");
    }

    #[test]
    fn test_loader_custom_settings() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [apps.main.custom]
            feature_enabled = true
            app_name = "test"
            max_items = 100
            "#,
        )
        .unwrap();

        let config = ConfigLoader::development()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();

        assert_eq!(config.custom.get_bool("feature_enabled"), Some(true));
        assert_eq!(config.custom.get_str("app_name"), Some("test"));
        assert_eq!(config.custom.get_int("max_items"), Some(100));
    }

    #[test]
    fn an_environment_table_in_fun_toml_holds_only_its_posture() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");
        std::fs::write(
            &config_path,
            "[apps.main]\nweb = { port = 3000 }\n\n[environments.production.apps.main]\nweb = { port = 8080 }\n",
        )
        .unwrap();
        let refused = ConfigLoader::production()
            .with_fundamental_toml(&config_path)
            .load();
        let error = refused.expect_err("an override in fun.toml");
        assert_eq!(error.error_code(), "CFG_033");
        assert!(error.to_string().contains("fun.production.toml"), "{error}");
    }

    #[test]
    fn test_loader_applies_custom_env_name_overrides() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [apps.main]
            web = { host = "127.0.0.1", port = 3000 }
            "#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("fun.devops.toml"),
            "[apps.main]\nweb = { host = \"0.0.0.0\", port = 9090 }\n",
        )
        .unwrap();

        let base_config = ConfigLoader::development()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();
        assert_eq!(base_config.server.host, "127.0.0.1");
        assert_eq!(base_config.server.port, 3000);

        let devops_config = ConfigLoader::new(Environment::Development)
            .with_env_name("devops")
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();
        assert_eq!(devops_config.server.host, "0.0.0.0");
        assert_eq!(devops_config.server.port, 9090);
    }

    #[test]
    fn test_merge_toml() {
        let mut base = toml::from_str::<toml::Value>(
            r#"
            [a]
            x = 1
            y = 2
            [b]
            z = 3
            "#,
        )
        .unwrap();

        let override_val = toml::from_str::<toml::Value>(
            r#"
            [a]
            y = 20
            [c]
            w = 4
            "#,
        )
        .unwrap();

        merge_toml(&mut base, override_val);

        let table = base.as_table().unwrap();
        let a = table.get("a").unwrap().as_table().unwrap();
        assert_eq!(a.get("x").unwrap().as_integer(), Some(1));
        assert_eq!(a.get("y").unwrap().as_integer(), Some(20)); // overridden
        assert!(table.get("b").is_some());
        assert!(table.get("c").is_some()); // added
    }

    #[test]
    fn test_typo_in_env_override_does_not_prevent_loading() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [apps.main]
            web = { host = "127.0.0.1", port = 3000 }
            "#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("fun.production.toml"),
            "[apps.main]\nweb = { host = \"0.0.0.1\" }\n",
        )
        .unwrap();

        let config = ConfigLoader::production()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();

        assert_eq!(config.server.host, "0.0.0.1");
        assert_eq!(config.server.port, 3000);
    }

    #[test]
    fn test_merge_main_surface_config_api() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [apps.main.api]
            port = 9001
            host = "0.0.0.0"
            cors_allowed_origins = ["http://localhost:5000"]
            "#,
        )
        .unwrap();

        let config = ConfigLoader::development()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();

        assert_eq!(config.api.port, 9001);
        assert_eq!(config.api.host, "0.0.0.0");
        assert_eq!(
            config.api.cors_allowed_origins,
            vec!["http://localhost:5000"]
        );
    }

    #[test]
    fn test_merge_main_surface_config_web() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [apps.main.web]
            port = 9002
            host = "0.0.0.0"
            tls_enabled = true
            "#,
        )
        .unwrap();

        let config = ConfigLoader::development()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();

        assert_eq!(config.server.port, 9002);
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(
            config.server.tls,
            crate::settings::TlsPosture::Enabled,
            "the legacy `tls_enabled = true` spelling still loads"
        );
    }

    #[test]
    fn test_app_scoped_database_promoted_to_top_level() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [apps.main.database.sql]
            url = "postgresql://main-db"
            "#,
        )
        .unwrap();

        let config = ConfigLoader::development()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();

        let sql = config.database.sql.as_ref().unwrap();
        assert_eq!(sql.url.expose(), "postgresql://main-db");
    }

    #[test]
    fn test_app_selection() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [apps.main.database.sql]
            url = "postgresql://main-db"

            [apps.admin.database.sql]
            url = "postgresql://admin-db"
            "#,
        )
        .unwrap();

        let main_config = ConfigLoader::development()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();
        let sql = main_config.database.sql.as_ref().unwrap();
        assert_eq!(sql.url.expose(), "postgresql://main-db");

        let admin_config = ConfigLoader::development()
            .with_app("admin")
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();
        let sql = admin_config.database.sql.as_ref().unwrap();
        assert_eq!(sql.url.expose(), "postgresql://admin-db");
    }

    #[test]
    fn test_no_main_app_uses_defaults() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [apps.admin]
            api = {}
            "#,
        )
        .unwrap();

        let config = ConfigLoader::development()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();

        assert_eq!(config.api.port, 16001);
        assert_eq!(config.server.port, 11001);
    }

    #[test]
    fn test_app_scoped_client_and_observability() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [apps.main.client]
            api_url = "http://127.0.0.1:9001"

            [apps.main.observability]
            log_level = "trace"
            log_format = "json"
            "#,
        )
        .unwrap();

        let config = ConfigLoader::development()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();

        assert_eq!(config.client.api_url, "http://127.0.0.1:9001");
        assert_eq!(config.observability.log_level, "trace");
        assert_eq!(config.observability.log_format, "json");
    }

    #[test]
    fn a_partial_observability_section_keeps_the_environment_s_defaults() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [apps.main.observability]
            log_format = "json"
            "#,
        )
        .unwrap();

        let deployed = ConfigLoader::new(fundamental_core::Environment::Production)
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();

        assert_eq!(
            deployed.observability.log_format, "json",
            "the stated key wins"
        );
        assert!(
            deployed.observability.otlp.enabled,
            "a key the file never mentions must keep its production default"
        );
        assert!(deployed.observability.tracing_enabled);
        assert!(
            (deployed.observability.sampling.ratio - 0.1).abs() < f64::EPSILON,
            "production samples a fraction; got {}",
            deployed.observability.sampling.ratio
        );
    }

    #[test]
    fn production_refuses_a_vault_key_that_declares_no_fingerprint() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");
        let write = |fingerprint: &str| {
            std::fs::write(
                &config_path,
                format!(
                    r#"
                    [encryption]
                    backend = "vault"
                    keys = [{{ id = "pii", encrypted_dek = "vault:v1:x", transit_key = "pii"{fingerprint} }}]
                    "#
                ),
            )
            .unwrap();
        };
        let load = || {
            ConfigLoader::new(Environment::Production)
                .with_fundamental_toml(&config_path)
                .load()
        };

        write("");
        let refused = load().expect_err("an undeclared vault key is refused in production");
        assert!(
            refused.to_string().contains("pii declare no fingerprint"),
            "{refused}"
        );

        write(&format!(", fingerprint = \"{}\"", "ab".repeat(32)));
        let loaded = load().expect("a declared key loads");
        assert_eq!(loaded.encryption.keys.len(), 1);
    }

    #[test]
    fn production_refuses_a_nats_credential_that_resolved_to_nothing() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");
        std::fs::write(
            &config_path,
            "[messaging.nats]\ntoken = \"${FUNDAMENTAL_TEST_UNSET_NATS_TOKEN:-}\"\n",
        )
        .unwrap();
        let load = |environment| {
            ConfigLoader::new(environment)
                .with_fundamental_toml(&config_path)
                .load()
        };

        let refused = load(Environment::Production)
            .expect_err("a configured credential that resolved to nothing is refused");
        assert!(
            refused.to_string().contains("token resolved to nothing"),
            "{refused}"
        );
        let loaded = load(Environment::Development).expect("a local run connects without one");
        assert!(loaded.messaging.nats.is_some());
    }

    #[test]
    fn test_app_scoped_messaging() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [apps.main.messaging.nats]
            url = "nats://localhost:4222"
            "#,
        )
        .unwrap();

        let config = ConfigLoader::development()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();

        let nats = config.messaging.nats.as_ref().unwrap();
        assert_eq!(nats.url, "nats://localhost:4222");
    }

    #[test]
    fn test_apps_config_preserves_app_typed_fields() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [apps.main]
            api = {}
            web = {}

            [apps.main.database.sql]
            url = "postgresql://main-db"

            [apps.main.client]
            api_url = "http://127.0.0.1:16001"

            [apps.admin]
            api = { package = "server-admin" }

            [apps.admin.database.sql]
            url = "postgresql://admin-db"
            "#,
        )
        .unwrap();

        let config = ConfigLoader::development()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();

        let main = config.apps.apps.get("main").unwrap();
        assert_eq!(main.surfaces.len(), 2);
        assert!(main.database.is_some());
        assert!(main.client.is_some());

        let admin = config.apps.apps.get("admin").unwrap();
        assert_eq!(admin.surfaces.len(), 1);
        assert!(admin.database.is_some());
        assert!(admin.client.is_none());
    }

    #[test]
    fn test_apps_key_populates_apps_registry() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [apps.main]
            api = {}
            web = {}

            [apps.main.database.sql]
            url = "postgresql://main-db"
            "#,
        )
        .unwrap();

        let config = ConfigLoader::development()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();

        let main = config.apps.apps.get("main").unwrap();
        assert_eq!(main.surfaces.len(), 2);
        assert!(main.database.is_some());
    }

    #[test]
    fn test_legacy_groups_key_still_loads() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [groups.main]
            api = {}

            [groups.main.database.sql]
            url = "postgresql://legacy-db"
            "#,
        )
        .unwrap();

        let config = ConfigLoader::development()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();

        assert!(config.apps.apps.contains_key("main"));
        let sql = config.database.sql.as_ref().unwrap();
        assert_eq!(sql.url.expose(), "postgresql://legacy-db");
    }
}

#[cfg(test)]
mod deprecation_tests {
    use super::*;
    use fundamental_core::Environment;
    use fundamental_core::env::EnvVars;
    use tempfile::TempDir;
    use tracing_test::traced_test;

    #[traced_test]
    #[test]
    fn legacy_groups_key_emits_deprecation_warning() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");

        std::fs::write(
            &config_path,
            r#"
            [groups.main]
            api = {}
            "#,
        )
        .unwrap();

        let _config = ConfigLoader::development()
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();

        assert!(logs_contain("use [apps.*] instead of [groups.*]"));
    }

    // Both variables are set to the default "main", so a leak into a parallel test is harmless.
    #[traced_test]
    #[test]
    fn env_var_selects_active_app_and_warns_on_legacy() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fun.toml");
        std::fs::write(
            &config_path,
            r#"
            [apps.main.database.sql]
            url = "postgresql://main-db"
            "#,
        )
        .unwrap();

        let config = ConfigLoader::new(Environment::Development)
            .reading_env_from(EnvVars::fixed([("FUNDAMENTAL_APP", "main")]))
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();
        assert_eq!(
            config.database.sql.as_ref().unwrap().url.expose(),
            "postgresql://main-db"
        );

        let config = ConfigLoader::new(Environment::Development)
            .reading_env_from(EnvVars::fixed([("FUNDAMENTAL_SURFACE_GROUP", "main")]))
            .with_fundamental_toml(&config_path)
            .load()
            .unwrap();
        assert_eq!(
            config.database.sql.as_ref().unwrap().url.expose(),
            "postgresql://main-db"
        );
        assert!(logs_contain(
            "use FUNDAMENTAL_APP instead of FUNDAMENTAL_SURFACE_GROUP"
        ));
    }
}
