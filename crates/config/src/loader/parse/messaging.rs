//! Parser for the `[messaging]` configuration section.

use crate::settings::{MessagingSettings, NatsSettings};
use crate::{ConfigError, ConfigResult};

/// Parses messaging settings from TOML.
pub(crate) fn parse_messaging_settings(value: &toml::Value) -> ConfigResult<MessagingSettings> {
    let table = value
        .as_table()
        .ok_or_else(|| ConfigError::invalid_value("messaging", "expected table"))?;

    let mut settings = MessagingSettings::default();

    if let Some(nats) = table.get("nats") {
        let nats_settings: NatsSettings =
            nats.clone().try_into().map_err(|e: toml::de::Error| {
                ConfigError::invalid_value("messaging.nats", e.to_string())
            })?;
        settings.nats = Some(nats_settings);
    }

    Ok(settings)
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use fundamental_core::Environment;

    use super::parse_messaging_settings;
    use crate::settings::{NatsAuth, NkeySigner, SecretRef, VaultKvRef};

    fn parsed(toml: &str) -> crate::ConfigResult<crate::settings::MessagingSettings> {
        let value: toml::Value = toml::from_str(toml).expect("valid TOML");
        parse_messaging_settings(&value)
    }

    #[test]
    fn a_configured_nats_password_reaches_the_settings() {
        let settings = parsed(
            r#"
            [nats]
            url = "nats://localhost:4222"
            username = "orders"
            password = "from-a-secret-file"
            "#,
        )
        .expect("a whole credential");
        let nats = settings.nats.expect("nats settings");
        let NatsAuth::UserPassword { username, password } = nats.auth else {
            panic!("expected a username and password, got {:?}", nats.auth);
        };
        assert_eq!(username, "orders");
        let SecretRef::Value(password) = password else {
            panic!("a written password is a value");
        };
        assert_eq!(password.expose(), "from-a-secret-file");
    }

    #[test]
    fn a_password_or_token_can_name_where_the_secret_store_holds_it() {
        let settings = parsed(
            r#"
            [nats]
            username = "mijn"
            password = { source = "vault_kv", mount = "a7d", path = "nats/mijn", key = "password" }
            secret_refresh_seconds = 60
            "#,
        )
        .expect("a referenced password");
        let nats = settings.nats.expect("nats settings");
        assert_eq!(nats.secret_refresh_seconds, 60);
        assert!(nats.requires_secret_manager());
        let NatsAuth::UserPassword {
            password: SecretRef::VaultKv(reference),
            ..
        } = &nats.auth
        else {
            panic!("expected a referenced password, got {:?}", nats.auth);
        };
        assert_eq!(
            reference,
            &VaultKvRef {
                mount: "a7d".into(),
                path: "nats/mijn".into(),
                key: "password".into()
            }
        );

        let token = parsed(
            "[nats]\ntoken = { source = \"vault_kv\", mount = \"secret\", path = \"nats/app\", key = \"token\" }\n",
        )
        .expect("a referenced token");
        assert!(matches!(
            token.nats.map(|nats| nats.auth),
            Some(NatsAuth::Token { token }) if token.is_reference()
        ));
    }

    #[test]
    fn an_nkey_names_its_public_key_and_its_signer() {
        let settings = parsed(
            r#"
            [nats]
            nkey = { public_key = "UAPZQH4MNJCOVEJFERB3NFSIROQ5RE7CGBEPKAZSB6QB7IQHBKXHZPVP", signer = { source = "vault_transit", mount = "transit", key = "nats-app" } }
            "#,
        )
        .expect("an nkey");
        let nats = settings.nats.expect("nats settings");
        let NatsAuth::NKey { public_key, signer } = &nats.auth else {
            panic!("expected an nkey, got {:?}", nats.auth);
        };
        assert_eq!(
            public_key.as_str(),
            "UAPZQH4MNJCOVEJFERB3NFSIROQ5RE7CGBEPKAZSB6QB7IQHBKXHZPVP"
        );
        assert_eq!(
            signer,
            &NkeySigner::VaultTransit {
                mount: "transit".into(),
                key: "nats-app".into()
            }
        );
        assert!(nats.requires_secret_manager());
    }

    #[test]
    fn a_token_and_a_credentials_file_are_read_too() {
        let token = parsed("[nats]\ntoken = \"tok\"\n").expect("a token");
        assert!(matches!(
            token.nats.map(|nats| nats.auth),
            Some(NatsAuth::Token { .. })
        ));
        let file = parsed("[nats]\ncredentials_path = \"/run/orders.creds\"\n").expect("a file");
        assert!(matches!(
            file.nats.map(|nats| nats.auth),
            Some(NatsAuth::CredentialsFile { .. })
        ));
    }

    #[test]
    fn a_credential_that_would_be_dropped_is_refused_at_load() {
        for (toml, why) in [
            ("[nats]\nusername = \"orders\"\n", "without its password"),
            ("[nats]\npassword = \"pw\"\n", "without a username"),
            (
                "[nats]\nusername = \"orders\"\npassword = \"pw\"\ntoken = \"tok\"\n",
                "configure one authentication method",
            ),
            ("[nats]\npasword = \"pw\"\n", "unknown field `pasword`"),
            (
                "[nats]\nurl = \"nats://bus:4222\"\ntls_ca_cert_path = \"/ca.pem\"\n",
                "tls_ca_cert_path set while TLS is off",
            ),
            (
                "[nats]\ntls_enabled = true\ntls_client_cert_path = \"/c.pem\"\n",
                "set together or not at all",
            ),
            (
                "[nats]\ntls_enabled = true\ntls_client_key_path = \"/c.key\"\n",
                "set together or not at all",
            ),
            (
                "[nats]\nusername = \"orders\"\npassword = { source = \"vault_kv\", mount = \"secret\", path = \"\", key = \"password\" }\n",
                "its path is not",
            ),
            (
                "[nats]\nnkey = { public_key = \"SUAMLK2ZNL35WSMW37E7UD4VZ7ELPKW7DHC3BWBSD2GCZ7IUQQXZIORRBU\", signer = { source = \"vault_transit\", mount = \"transit\", key = \"k\" } }\n",
                "this is a seed",
            ),
            (
                "[nats]\nsecret_refresh_seconds = 0\n",
                "between 1 and 86400",
            ),
            (
                "[nats]\nnkey = { public_key = \"UAPZQH4MNJCOVEJFERB3NFSIROQ5RE7CGBEPKAZSB6QB7IQHBKXHZPVP\", signer = { source = \"vault_kv\", mount = \"transit\", key = \"k\" } }\n",
                "unknown variant `vault_kv`",
            ),
            (
                "[nats]\nnkey = { public_key = \"UAPZQH4MNJCOVEJFERB3NFSIROQ5RE7CGBEPKAZSB6QB7IQHBKXHZPVP\", signer = { source = \"vault_transit\", mount = \"transit\", key = \"k\", seed = \"S\" } }\n",
                "unknown field `seed`",
            ),
            (
                "[nats]\nnkey = { public_key = \"UAPZQH4MNJCOVEJFERB3NFSIROQ5RE7CGBEPKAZSB6QB7IQHBKXHZPVP\", signer = { source = \"vault_transit\", mount = \"transit\", key = \"\" } }\n",
                "key is 1-128 characters",
            ),
            (
                "[nats]\nsecret_refresh_seconds = 86401\n",
                "between 1 and 86400",
            ),
        ] {
            let refused = parsed(toml).expect_err("a credential is never silently dropped");
            assert_eq!(refused.error_code(), "CFG_004", "{toml}");
            assert!(refused.to_string().contains(why), "{toml}: {refused}");
        }
    }

    #[test]
    fn tls_material_is_applied_by_either_switch() {
        for toml in [
            "[nats]\ntls_enabled = true\ntls_ca_cert_path = \"/ca.pem\"\n",
            "[nats]\nurl = \"nats://a:4222, tls://b:4222\"\ntls_ca_cert_path = \"/ca.pem\"\n",
        ] {
            let settings = parsed(toml).expect("TLS material with TLS on");
            let nats = settings.nats.expect("a nats section");
            assert!(nats.requests_tls(), "{toml}");
            assert_eq!(nats.tls_ca_cert_path.as_deref(), Some("/ca.pem"));
        }
    }

    #[test]
    fn a_blank_credential_is_remembered_rather_than_forgotten() {
        let settings = parsed("[nats]\ntoken = \"\"\nusername = \"\"\n")
            .expect("a blank credential configures none");
        let nats = settings.nats.expect("a nats section");
        assert!(matches!(nats.auth, crate::settings::NatsAuth::None));
        assert_eq!(nats.blank_credentials, ["username", "token"]);

        for deployed in [Environment::Staging, Environment::Production] {
            let refused = nats.require_deployed_credentials(deployed).expect_err(
                "a deployment connects only with the credential it was configured with",
            );
            assert_eq!(refused.error_code(), "CFG_004");
            assert!(
                refused
                    .to_string()
                    .contains("username, token resolved to nothing"),
                "{refused}"
            );
        }
        nats.require_deployed_credentials(Environment::Development)
            .expect("a local run may leave a credential unset");
    }

    #[test]
    fn a_protected_environment_refuses_a_plain_nats_secret() {
        for toml in [
            "[nats]\nurl = \"tls://bus:4222\"\nusername = \"orders\"\npassword = \"pw\"\n",
            "[nats]\nurl = \"tls://bus:4222\"\ntoken = \"tok\"\n",
        ] {
            let nats = parsed(toml).expect("parses").nats.expect("a nats section");
            for deployed in [Environment::Staging, Environment::Production] {
                let refused = nats
                    .require_deployed_credentials(deployed)
                    .expect_err("a deployed secret comes from the secret store");
                assert_eq!(refused.error_code(), "CFG_004");
                assert!(
                    refused.to_string().contains("is a plain value"),
                    "{refused}"
                );
            }
            nats.require_deployed_credentials(Environment::Development)
                .expect("a local run may write its password");
        }
    }

    #[test]
    fn a_deployed_environment_accepts_every_credential_that_is_not_a_plain_secret() {
        for toml in [
            "[nats]\nurl = \"tls://bus:4222\"\nusername = \"orders\"\npassword = { source = \"vault_kv\", mount = \"secret\", path = \"nats/orders\", key = \"password\" }\n",
            "[nats]\nurl = \"tls://bus:4222\"\nnkey = { public_key = \"UAPZQH4MNJCOVEJFERB3NFSIROQ5RE7CGBEPKAZSB6QB7IQHBKXHZPVP\", signer = { source = \"vault_transit\", mount = \"transit\", key = \"k\" } }\n",
            "[nats]\nurl = \"tls://bus:4222\"\ncredentials_path = \"/run/orders.creds\"\n",
            "[nats]\nurl = \"tls://bus:4222\"\ntls_client_cert_path = \"/c.pem\"\ntls_client_key_path = \"/c.key\"\n",
        ] {
            let nats = parsed(toml).expect("parses").nats.expect("a nats section");
            nats.require_deployed_credentials(Environment::Production)
                .unwrap_or_else(|refused| panic!("{toml}: {refused}"));
        }
    }

    #[test]
    fn a_deployed_environment_refuses_a_credential_over_a_plaintext_hop() {
        let referenced = "username = \"orders\"\npassword = { source = \"vault_kv\", mount = \"secret\", path = \"nats/orders\", key = \"password\" }\n";
        let over = |url: &str| {
            parsed(&format!("[nats]\nurl = \"{url}\"\n{referenced}"))
                .expect("parses")
                .nats
                .expect("a nats section")
        };
        for url in ["nats://bus:4222", "nats://127.0.0.1:4222, nats://bus:4222"] {
            let refused = over(url)
                .require_deployed_credentials(Environment::Staging)
                .expect_err("a password never crosses the network in the clear");
            assert!(
                refused.to_string().contains("in the clear"),
                "{url}: {refused}"
            );
        }
        for credential in [
            "nkey = { public_key = \"UAPZQH4MNJCOVEJFERB3NFSIROQ5RE7CGBEPKAZSB6QB7IQHBKXHZPVP\", signer = { source = \"vault_transit\", mount = \"transit\", key = \"k\" } }\n",
            "credentials_path = \"/run/orders.creds\"\n",
        ] {
            let nats = parsed(&format!("[nats]\nurl = \"nats://bus:4222\"\n{credential}"))
                .expect("parses")
                .nats
                .expect("a nats section");
            let refused = nats
                .require_deployed_credentials(Environment::Production)
                .expect_err("a signature relayed in the clear is a session taken");
            assert!(
                refused.to_string().contains("in the clear"),
                "{credential}: {refused}"
            );
        }
        for url in ["nats://127.0.0.1:4222", "tls://bus:4222"] {
            over(url)
                .require_deployed_credentials(Environment::Production)
                .unwrap_or_else(|refused| panic!("{url}: {refused}"));
        }
    }

    #[test]
    fn a_dumped_configuration_never_carries_the_password() {
        let settings = parsed("[nats]\nusername = \"orders\"\npassword = \"from-a-secret-file\"\n")
            .expect("a whole credential");
        let dumped = toml::to_string(&settings).expect("serialisable");
        assert!(!dumped.contains("from-a-secret-file"), "{dumped}");
        assert!(dumped.contains("[REDACTED]"), "{dumped}");
    }
}
