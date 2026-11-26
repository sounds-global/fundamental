//! Loading and reading Fundamental configuration: `cargo run --example load_config -p fundamental_config`.

use fundamental_config::{ConfigLoader, ConfigResult, FundamentalConfig};
use serde::Deserialize;

fn main() {
    if let Err(err) = run() {
        eprintln!("configuration error: {err}");
        std::process::exit(1);
    }
}

#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn run() -> ConfigResult<()> {
    let defaults = FundamentalConfig::default();
    println!("== Defaults ==");
    println!("  environment : {}", defaults.environment_name());
    println!("  api          : {}", defaults.api_url());
    println!(
        "  ssr server   : {}:{}",
        defaults.server.host, defaults.server.port
    );
    println!();

    let dir = tempfile::tempdir().map_err(|e| {
        fundamental_config::ConfigError::validation(format!("failed to create temp dir: {e}"))
    })?;
    let config_path = dir.path().join("fun.toml");
    std::fs::write(
        &config_path,
        r#"
        [project]
        name = "load-config-example"
        version = "0.1.0"

        # Apps are self-contained deployment units. The active app defaults to
        # "main"; its config sections are promoted to the top level.
        [apps.main]
        web = { host = "0.0.0.0", port = 8080, render = "ssr" }
        api = { port = 9000 }

        [apps.main.client]
        api_url = "http://127.0.0.1:9000"
        timeout_secs = 15

        [apps.main.observability]
        log_level = "info"
        log_format = "json"

        # Application-specific values live under [custom].
        [apps.main.custom]
        feature_flags.new_checkout = true
        max_items = 250
        payment.provider = "stripe"
        payment.sandbox = true
        "#,
    )
    .map_err(|e| {
        fundamental_config::ConfigError::validation(format!("failed to write config: {e}"))
    })?;

    let config = ConfigLoader::development()
        .with_app("main")
        .with_fundamental_toml(&config_path)
        .with_env_vars()
        .load()?;

    println!("== Loaded from fun.toml ==");
    println!(
        "  project      : {} v{}",
        config.project.name, config.project.version
    );
    println!("  environment  : {}", config.environment_name());
    println!(
        "  ssr server   : {}:{}",
        config.server.host, config.server.port
    );
    println!("  api          : {}", config.api_url());
    println!("  client url   : {}", config.client.api_url);
    println!("  client timeout: {}s", config.client.timeout_secs);
    println!(
        "  observability: level={} format={}",
        config.observability.log_level, config.observability.log_format
    );
    println!();

    println!("== Custom settings ==");
    let new_checkout = config
        .custom
        .get_bool("feature_flags.new_checkout")
        .unwrap_or(false);
    let max_items = config.custom.get_int("max_items").unwrap_or(0);
    println!("  feature_flags.new_checkout = {new_checkout}");
    println!("  max_items                  = {max_items}");

    #[derive(Debug, Deserialize)]
    #[expect(
        dead_code,
        reason = "the example reads the fields only through `Debug`"
    )]
    struct PaymentConfig {
        provider: String,
        sandbox: bool,
    }
    let payment: PaymentConfig = config.custom.section("payment")?;
    println!("  payment                    = {payment:?}");
    println!();

    match config.validate() {
        Ok(()) => println!(
            "configuration is valid for the '{}' environment",
            config.environment_name()
        ),
        Err(e) => println!("configuration failed validation: {e}"),
    }

    Ok(())
}
